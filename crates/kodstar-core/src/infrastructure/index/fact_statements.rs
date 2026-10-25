// Purpose: fact 저장에 필요한 고정 SQL 11개의 메모리·binding 수명 소유.
// Pattern: Prepared Statements.
// Usage: FactWriter가 transaction 동안 하나를 소유하고 파일들에서 재사용.
// Related: crates/kodstar-core/src/infrastructure/index/facts.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use rusqlite::{Connection, Params, Statement, StatementStatus};

#[derive(Clone, Copy)]
pub(super) enum Write {
    Scope,
    Definition,
    Binding,
    Usage,
    Ordinal,
    LocalSymbol,
    DeclaredModule,
    Module,
    UpdateScope,
    UpdateDefinition,
    UpdateFunctionEnd,
}

pub(super) struct Statements<'conn> {
    statements: [Statement<'conn>; 11],
    bytes: usize,
    limit: usize,
}
impl<'conn> Statements<'conn> {
    pub(super) fn new(connection: &'conn Connection) -> Result<Self> {
        let mut statements = Vec::with_capacity(11);
        for sql in [
            "INSERT INTO scopes(file_id,local_id,parent,start_byte,end_byte)
            VALUES(?1,?2,?3,?4,?5)",
            "INSERT INTO symbols_rows(file_id,local_id,scope_id,syntax_key,name,name_key,syntax_qualified_name,syntax_kind,signature,condition,owner_id,exported,start_byte,end_byte,line,column_byte,end_line,end_column_byte,language)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            "INSERT INTO bindings_rows(file_id,scope_id,name,name_key,target_local,module_name,imported_name,type_name,kind,start_byte,end_byte,namespace)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            "INSERT INTO facts_rows(file_id,source_id,scope_id,expression,relation,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte,language)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            "SELECT COUNT(*) FROM symbols_rows
            WHERE file_id=?1 AND qualified_id=?2 AND kind_value=?3 AND signature=?4 AND condition IS ?5",
            "SELECT id FROM symbols_rows
            WHERE file_id=?1 AND local_id=?2",
            "UPDATE files_rows SET declared_module=?1
            WHERE id=?2",
            "UPDATE files_rows SET module=?1,declared_module=?1
            WHERE id=?2",
            "UPDATE scopes SET start_byte=?1,end_byte=?2 WHERE file_id=?3 AND local_id=?4",
            "UPDATE symbols_rows SET start_byte=?1,end_byte=?2,line=?3,column_byte=?4,end_line=?5,end_column_byte=?6 WHERE file_id=?7 AND local_id=?8",
            "UPDATE bindings_rows SET end_byte=?1 WHERE file_id=?2 AND target_local=?3 AND kind IN (SELECT id FROM atoms WHERE value IN ('function','go_signature'))",
        ] {
            statements.push(connection.prepare(sql)?);
        }
        let bytes = statements
            .iter()
            .map(|s| s.get_status(StatementStatus::MemUsed) as usize)
            .sum();
        let limit = connection.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize;
        if bytes > limit {
            return Err(limit_error());
        }
        Ok(Self {
            statements: statements
                .try_into()
                .map_err(|_| Error::operation("invalid fact statement count"))?,
            bytes,
            limit,
        })
    }
    pub(super) fn execute<P: Params>(&mut self, kind: Write, parameters: P) -> Result<()> {
        let index = kind as usize;
        let statement = &mut self.statements[index];
        let before = statement.get_status(StatementStatus::MemUsed) as usize;
        let result = statement.execute(parameters).map(|_| ());
        self.finish(index, before, result)
    }
    pub(super) fn integer<P: Params>(&mut self, kind: Write, parameters: P) -> Result<i64> {
        let index = kind as usize;
        let statement = &mut self.statements[index];
        let before = statement.get_status(StatementStatus::MemUsed) as usize;
        let result = statement.query_row(parameters, |r| r.get(0));
        self.finish(index, before, result)
    }
    fn finish<T>(&mut self, index: usize, before: usize, result: rusqlite::Result<T>) -> Result<T> {
        let statement = &mut self.statements[index];
        statement.clear_bindings();
        self.bytes = self
            .bytes
            .saturating_sub(before)
            .saturating_add(statement.get_status(StatementStatus::MemUsed) as usize);
        let value = result?;
        if self.bytes > self.limit {
            return Err(limit_error());
        }
        Ok(value)
    }
}
fn limit_error() -> Error {
    Error::new(
        ErrorKind::Limit,
        "prepared fact statements exceed the working-memory read budget",
    )
}
