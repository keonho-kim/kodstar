// Purpose: 객체 행의 준비된 조회와 snapshot 안의 작은 반복 객체 재사용.
// Pattern: Bounded Reader.
// Usage: 관계 스트림이 하나를 소유하며 단일 객체 조회도 같은 행 계약 사용.
// Related: crates/kodstar-core/src/infrastructure/index/mod.rs, crates/kodstar-core/src/application/query/relationship_rows.rs.

use super::{SYMBOL_COLUMNS, complete_symbol, symbol_from_row};
use crate::domain::diagnostics::Result;
use crate::domain::symbols::Symbol;
use rusqlite::{Connection, Statement};

pub(crate) struct SymbolReader<'conn> {
    connection: &'conn Connection,
    statement: Statement<'conn>,
    recent: [Option<(i64, Symbol)>; 2],
    next: usize,
    entry_limit: usize,
}

impl<'conn> SymbolReader<'conn> {
    pub(crate) fn new(connection: &'conn Connection) -> Result<Self> {
        let entry_limit =
            (connection.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize / 16)
                .min(32 * 1024);
        Ok(Self {
            connection,
            statement: connection.prepare(&format!(
                "SELECT {SYMBOL_COLUMNS} FROM symbols s JOIN files f ON f.id=s.file_id WHERE s.id=?1"
            ))?,
            recent: [None, None],
            next: 0,
            entry_limit,
        })
    }

    pub(crate) fn read(&mut self, id: i64) -> Result<Symbol> {
        if let Some((_, symbol)) = self.recent.iter().flatten().find(|(key, _)| *key == id) {
            return Ok(symbol.clone());
        }
        let (mut value, definitions) = self
            .statement
            .query_row([id], |row| Ok((symbol_from_row(row)?, row.get(20)?)))?;
        complete_symbol(self.connection, &mut value, definitions)?;
        if owned_bytes(&value) <= self.entry_limit {
            self.recent[self.next] = Some((id, value.clone()));
            self.next = (self.next + 1) % self.recent.len();
        }
        Ok(value)
    }
}

pub(crate) fn symbol(connection: &Connection, id: i64) -> Result<Symbol> {
    SymbolReader::new(connection)?.read(id)
}

fn owned_bytes(symbol: &Symbol) -> usize {
    let strings = [
        &symbol.logical_key,
        &symbol.name,
        &symbol.qualified_name,
        &symbol.kind,
        &symbol.language,
        &symbol.module,
        &symbol.package,
        &symbol.file,
        &symbol.signature,
    ];
    std::mem::size_of::<Symbol>()
        + strings.iter().map(|s| s.capacity()).sum::<usize>()
        + symbol.condition.as_ref().map_or(0, String::capacity)
        + symbol
            .file_encoding
            .as_ref()
            .map_or(0, |p| p.bytes_base64.capacity())
        + symbol.definitions.capacity()
            * std::mem::size_of::<crate::domain::symbols::DefinitionLocation>()
        + symbol
            .definitions
            .iter()
            .map(|d| {
                d.file.capacity()
                    + d.file_encoding
                        .as_ref()
                        .map_or(0, |p| p.bytes_base64.capacity())
            })
            .sum::<usize>()
}
