// Purpose: 준비된 관계 SQL과 제한된 객체 reader로 공개 관계 행 복원.
// Pattern: Storage Reader.
// Usage: 관계 스트림이 한 snapshot 동안 소유하고 행마다 read 호출.
// Related: crates/kodstar-core/src/application/query/relationships.rs, crates/kodstar-core/src/infrastructure/index/reader.rs.

use crate::domain::diagnostics::Result;
use crate::domain::results::{Relationship, UseSite};
use crate::domain::source::SourceSpan;
use crate::infrastructure::cache::paths::{encoding, path_from_bytes};
use crate::infrastructure::index::SymbolReader;
use rusqlite::{Connection, Statement};

pub(super) struct Reader<'conn> {
    statement: Statement<'conn>,
    symbols: SymbolReader<'conn>,
}
impl<'conn> Reader<'conn> {
    pub(super) fn new(connection: &'conn Connection) -> Result<Self> {
        Ok(Self {
            statement: connection.prepare("SELECT r.source_id,r.target_id,r.external_key,f.relation,r.resolution,r.rule,r.reason,
                fi.display_path,fi.path,f.start_byte,f.end_byte,f.line,f.column_byte,f.end_line,
                f.end_column_byte,f.condition,f.type_only,f.expression
             FROM relationships r JOIN facts f ON f.id=r.fact_id
             JOIN files fi ON fi.id=f.file_id WHERE r.id=?1")?,
            symbols: SymbolReader::new(connection)?,
        })
    }
    pub(super) fn read(
        &mut self,
        id: i64,
        direction: &'static str,
        depth: usize,
        count: u64,
    ) -> Result<Relationship> {
        let stored = self.statement.query_row([id], |row| {
            Ok(StoredRelationship {
                source: row.get(0)?,
                target: row.get(1)?,
                external: row.get(2)?,
                relation: row.get(3)?,
                resolution: row.get(4)?,
                rule: row.get(5)?,
                reason: row.get(6)?,
                file: row.get(7)?,
                raw: row.get(8)?,
                span: SourceSpan {
                    start_byte: row.get(9)?,
                    end_byte: row.get(10)?,
                    line: row.get(11)?,
                    column: row.get(12)?,
                    end_line: row.get(13)?,
                    end_column: row.get(14)?,
                },
                condition: row.get(15)?,
                type_only: row.get(16)?,
                expression: row.get(17)?,
            })
        })?;
        let expression = if let Some(raw) = stored
            .expression
            .strip_prefix("@lisp-symbol:")
            .or_else(|| stored.expression.strip_prefix("@php-new:"))
            .or_else(|| stored.expression.strip_prefix("@constructor:"))
            .or_else(|| stored.expression.strip_prefix("@php-import-type:"))
            .or_else(|| stored.expression.strip_prefix("@php-import-function:"))
            .or_else(|| stored.expression.strip_prefix("@php-import-value:"))
        {
            raw.to_owned()
        } else {
            stored.expression
        };
        Ok(Relationship {
            source: self.symbols.read(stored.source)?,
            target: stored
                .target
                .map(|target| self.symbols.read(target))
                .transpose()?,
            external: stored.external,
            relation: stored.relation,
            expression,
            direction,
            resolution: stored.resolution,
            rule: stored.rule,
            reason: stored.reason,
            site: UseSite {
                file: stored.file,
                span: stored.span,
                path_encoding: encoding(&path_from_bytes(&stored.raw)?),
            },
            count,
            depth,
            condition: stored.condition,
            type_only: stored.type_only,
            provider: "kodstar",
            provider_version: crate::VERSION,
        })
    }
}

struct StoredRelationship {
    source: i64,
    target: Option<i64>,
    external: Option<String>,
    relation: String,
    resolution: String,
    rule: String,
    reason: Option<String>,
    file: String,
    raw: Vec<u8>,
    span: SourceSpan,
    condition: Option<String>,
    type_only: bool,
    expression: String,
}
