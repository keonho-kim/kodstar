// Purpose: 준비된 관계 SQL과 제한된 객체 reader로 공개 관계 행 복원.
// Pattern: Storage Reader.
// Usage: 관계 스트림이 한 snapshot 동안 소유하고 행마다 read 호출.
// Related: crates/kodstar-core/src/application/query/relationships.rs, crates/kodstar-core/src/infrastructure/index/reader.rs, crates/kodstar-core/src/application/query/relationship_rows/batch.rs.

use crate::domain::diagnostics::Result;
use crate::domain::results::{Relationship, UseSite};
use crate::domain::source::SourceSpan;
use crate::infrastructure::cache::paths::{encoding, path_from_bytes};
use crate::infrastructure::index::SymbolReader;
use crate::infrastructure::index::relations::Relation;
use rusqlite::{Connection, Statement};
use std::collections::VecDeque;
use std::sync::Arc;

mod batch;

pub(super) struct Reader<'conn> {
    statement: Statement<'conn>,
    fact: Option<(i64, Arc<StoredRelationship>)>,
    fact_limit: usize,
    symbols: SymbolReader<'conn>,
    relations: crate::infrastructure::index::relations::Reader<'conn>,
    connection: &'conn Connection,
    batch_statement: Option<Statement<'conn>>,
    facts: Vec<(i64, Arc<StoredRelationship>)>,
    selected: VecDeque<Relation>,
    pending: Option<Relation>,
}
impl<'conn> Reader<'conn> {
    pub(super) fn new(connection: &'conn Connection) -> Result<Self> {
        Ok(Self {
            fact: None,
            connection,
            batch_statement: None,
            facts: Vec::new(),
            selected: VecDeque::new(),
            pending: None,
            fact_limit: (connection.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize
                / 64)
                .min(16384),
            statement: connection.prepare(
                "SELECT f.relation,
                fi.display_path,fi.path,f.start_byte,f.end_byte,f.line,f.column_byte,f.end_line,
                f.end_column_byte,f.condition,f.type_only,f.expression
             FROM facts f JOIN files fi ON fi.id=f.file_id WHERE f.id=?1",
            )?,
            symbols: SymbolReader::new(connection)?,
            relations: crate::infrastructure::index::relations::Reader::new(connection)?,
        })
    }
    pub(super) fn read(
        &mut self,
        id: i64,
        direction: &'static str,
        depth: usize,
        count: u64,
    ) -> Result<Relationship> {
        let relation = if self.selected.front().is_some_and(|row| row.id == id) {
            self.selected.pop_front().unwrap()
        } else if self.pending.as_ref().is_some_and(|row| row.id == id) {
            self.pending.take().unwrap()
        } else {
            self.relations.read(id)?
        };
        let stored = self.fact(relation.fact)?;
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
            crate::domain::resolve::java::display(&stored.expression).into_owned()
        };
        Ok(Relationship {
            source: self.symbols.read(relation.source)?,
            target: relation
                .target
                .map(|target| self.symbols.read(target))
                .transpose()?,
            external: relation.external,
            relation: stored.relation.clone(),
            expression,
            direction,
            resolution: relation.state.as_str().to_owned(),
            rule: self.relations.text(relation.rule)?,
            reason: relation
                .reason
                .map(|id| self.relations.text(id))
                .transpose()?,
            site: UseSite {
                file: stored.file.clone(),
                span: stored.span,
                path_encoding: stored.path_encoding.clone(),
            },
            count,
            depth,
            condition: stored.condition.clone(),
            type_only: stored.type_only,
            provider: "kodstar",
            provider_version: crate::VERSION,
        })
    }
    fn fact(&mut self, id: i64) -> Result<Arc<StoredRelationship>> {
        if let Ok(at) = self.facts.binary_search_by_key(&id, |(key, _)| *key) {
            return Ok(Arc::clone(&self.facts[at].1));
        }
        if let Some((fact, stored)) = &self.fact
            && *fact == id
        {
            return Ok(Arc::clone(stored));
        }
        let (stored, raw) = self
            .statement
            .query_row([id], StoredRelationship::from_row)?;
        let stored = stored.with_path(raw)?;
        self.fact = (stored.owned_bytes() <= self.fact_limit).then(|| (id, Arc::clone(&stored)));
        Ok(stored)
    }
}

impl StoredRelationship {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Self, Vec<u8>)> {
        Ok((
            Self {
                relation: row.get(0)?,
                file: row.get(1)?,
                path_encoding: None,
                span: SourceSpan {
                    start_byte: row.get(3)?,
                    end_byte: row.get(4)?,
                    line: row.get(5)?,
                    column: row.get(6)?,
                    end_line: row.get(7)?,
                    end_column: row.get(8)?,
                },
                condition: row.get(9)?,
                type_only: row.get(10)?,
                expression: row.get(11)?,
            },
            row.get(2)?,
        ))
    }

    fn with_path(mut self, raw: Vec<u8>) -> Result<Arc<Self>> {
        self.path_encoding = if std::str::from_utf8(&raw).is_ok() {
            None
        } else {
            encoding(&path_from_bytes(&raw)?)
        };
        Ok(Arc::new(self))
    }

    fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.file.capacity()
            + self.relation.capacity()
            + self.expression.capacity()
            + self.condition.as_ref().map_or(0, String::capacity)
            + self
                .path_encoding
                .as_ref()
                .map_or(0, |e| e.bytes_base64.capacity())
    }
}

struct StoredRelationship {
    relation: String,
    file: String,
    path_encoding: Option<crate::domain::source::PathEncoding>,
    span: SourceSpan,
    condition: Option<String>,
    type_only: bool,
    expression: String,
}
