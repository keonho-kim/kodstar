// Purpose: 객체 행의 제한된 배치 복원과 snapshot 안의 반복 객체 공유.
// Pattern: Bounded Reader.
// Usage: 관계 스트림이 하나를 소유하며 단일 객체 조회도 같은 행 계약 사용.
// Related: crates/kodstar-core/src/infrastructure/index/mod.rs, crates/kodstar-core/src/application/query/relationship_rows.rs, crates/kodstar-core/src/infrastructure/index/reader/batch.sql.

use super::{SYMBOL_COLUMNS, complete_symbol, symbol_from_row};
use crate::domain::diagnostics::Result;
use crate::domain::symbols::Symbol;
use rusqlite::{Connection, Statement};
use std::sync::Arc;

pub(crate) const BATCH_IDS: &str = include_str!("reader/batch.sql");

pub(crate) struct SymbolReader<'conn> {
    connection: &'conn Connection,
    statement: Statement<'conn>,
    recent: [Option<(i64, Arc<Symbol>)>; 2],
    next: usize,
    entry_limit: usize,
    batch: Vec<(i64, Arc<Symbol>)>,
    batch_statement: Option<Statement<'conn>>,
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
            batch: Vec::new(),
            batch_statement: None,
        })
    }

    pub(crate) fn read(&mut self, id: i64) -> Result<Arc<Symbol>> {
        if let Ok(at) = self.batch.binary_search_by_key(&id, |(key, _)| *key) {
            let value = Arc::clone(&self.batch[at].1);
            self.remember(id, &value);
            return Ok(value);
        }
        if let Some((at, symbol)) = self.recent.iter().enumerate().find_map(|(at, entry)| {
            entry
                .as_ref()
                .filter(|(key, _)| *key == id)
                .map(|(_, symbol)| (at, symbol))
        }) {
            let symbol = Arc::clone(symbol);
            self.next = (at + 1) % self.recent.len();
            return Ok(symbol);
        }
        let value = Arc::new(self.load(id)?);
        self.remember(id, &value);
        Ok(value)
    }

    fn remember(&mut self, id: i64, value: &Arc<Symbol>) {
        if let Some(at) = self
            .recent
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|(key, _)| *key == id))
        {
            self.next = (at + 1) % self.recent.len();
        } else if owned_bytes(value) + 2 * std::mem::size_of::<usize>() <= self.entry_limit {
            self.recent[self.next] = Some((id, Arc::clone(value)));
            self.next = (self.next + 1) % self.recent.len();
        }
    }

    fn load(&mut self, id: i64) -> Result<Symbol> {
        let (mut value, definitions) = self
            .statement
            .query_row([id], |row| Ok((symbol_from_row(row)?, row.get(20)?)))?;
        complete_symbol(self.connection, &mut value, definitions)?;
        Ok(value)
    }

    pub(crate) fn prefetch(
        &mut self,
        ids: &[i64],
        cancel: &crate::domain::cancellation::Cancellation,
    ) -> Result<()> {
        self.batch.clear();
        if ids.len() < 4 {
            return Ok(());
        }
        if self.batch_statement.is_none() {
            self.batch_statement =
                Some(self.connection.prepare(&format!(
                "{BATCH_IDS} SELECT {SYMBOL_COLUMNS} FROM symbols s JOIN files f ON f.id=s.file_id
                 WHERE s.id IN (SELECT id FROM batch) ORDER BY s.id"))?);
        }
        let parameters = std::array::from_fn::<_, 64, _>(|at| ids.get(at).copied());
        let mut rows = self
            .batch_statement
            .as_mut()
            .unwrap()
            .query(rusqlite::params_from_iter(parameters))?;
        let mut used = 64 * std::mem::size_of::<(i64, Arc<Symbol>)>();
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let id: i64 = row.get(0)?;
            if let Some(value) = self
                .recent
                .iter()
                .flatten()
                .find(|(key, _)| *key == id)
                .map(|(_, value)| Arc::clone(value))
            {
                used = used.saturating_add(owned_bytes(&value) + 2 * std::mem::size_of::<usize>());
                if used > self.entry_limit {
                    break;
                }
                self.batch.push((id, value));
                continue;
            }
            let mut symbol = symbol_from_row(row)?;
            complete_symbol(self.connection, &mut symbol, row.get(20)?)?;
            used = used.saturating_add(owned_bytes(&symbol) + 2 * std::mem::size_of::<usize>());
            if used > self.entry_limit {
                break;
            }
            self.batch.push((symbol.id, Arc::new(symbol)));
        }
        Ok(())
    }
}

pub(crate) fn symbol(connection: &Connection, id: i64) -> Result<Symbol> {
    SymbolReader::new(connection)?.load(id)
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
