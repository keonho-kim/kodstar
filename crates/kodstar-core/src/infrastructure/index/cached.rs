// Purpose: 작업별 SQL statement 재사용과 총 보유 바이트 제한.
// Pattern: Resource Pool.
// Usage: pipeline·설정·resolve가 각 작업의 연결을 빌려 풀을 소유.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/infrastructure/index/runtime.rs.

use rusqlite::{Connection, Params, Row, Statement, StatementStatus};
use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};

#[cfg(test)]
mod tests;

struct Entry<'conn> {
    sql: String,
    statement: Statement<'conn>,
    bytes: usize,
}

pub(crate) struct Sql<'conn> {
    connection: &'conn Connection,
    idle: RefCell<Vec<Entry<'conn>>>,
    idle_bytes: Cell<usize>,
    bytes: Cell<usize>,
    limit: usize,
}

pub(crate) struct ReusedStatement<'pool, 'conn> {
    pool: &'pool Sql<'conn>,
    // Only Drop takes the entry; Deref always observes an owned active statement.
    entry: Option<Entry<'conn>>,
}

impl<'conn> Sql<'conn> {
    pub(crate) fn new(connection: &'conn Connection) -> rusqlite::Result<Self> {
        let limit = connection.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize;
        Ok(Self {
            connection,
            idle: RefCell::default(),
            idle_bytes: Cell::new(0),
            bytes: Cell::new(0),
            limit,
        })
    }

    pub(crate) fn connection(&self) -> &'conn Connection {
        self.connection
    }

    pub(crate) fn cached_prepare(&self, sql: &str) -> rusqlite::Result<ReusedStatement<'_, 'conn>> {
        let mut idle = self.idle.borrow_mut();
        if let Some(index) = idle.iter().position(|entry| entry.sql == sql) {
            let entry = idle.remove(index);
            self.idle_bytes
                .set(self.idle_bytes.get().saturating_sub(entry.bytes));
            return Ok(ReusedStatement {
                pool: self,
                entry: Some(entry),
            });
        }
        let statement = self.connection.prepare(sql)?;
        let bytes = statement.get_status(StatementStatus::MemUsed) as usize + sql.len();
        while self.bytes.get().saturating_add(bytes) > self.limit && !idle.is_empty() {
            let old = idle.remove(0);
            self.bytes.set(self.bytes.get().saturating_sub(old.bytes));
            self.idle_bytes
                .set(self.idle_bytes.get().saturating_sub(old.bytes));
        }
        if self.bytes.get().saturating_add(bytes) > self.limit {
            return Err(too_large());
        }
        self.bytes.set(self.bytes.get() + bytes);
        Ok(ReusedStatement {
            pool: self,
            entry: Some(Entry {
                sql: sql.into(),
                statement,
                bytes,
            }),
        })
    }

    pub(crate) fn cached_execute<P: Params>(
        &self,
        sql: &str,
        params: P,
    ) -> rusqlite::Result<usize> {
        let mut statement = self.cached_prepare(sql)?;
        let result = statement.execute(params);
        statement.finish(result)
    }

    pub(crate) fn cached_row<T, P, F>(&self, sql: &str, params: P, map: F) -> rusqlite::Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> rusqlite::Result<T>,
    {
        let mut statement = self.cached_prepare(sql)?;
        let result = statement.query_row(params, map);
        statement.finish(result)
    }
}

impl Deref for Sql<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.connection
    }
}

impl<'conn> Deref for ReusedStatement<'_, 'conn> {
    type Target = Statement<'conn>;
    fn deref(&self) -> &Self::Target {
        &self
            .entry
            .as_ref()
            .expect("statement remains owned until drop")
            .statement
    }
}
impl DerefMut for ReusedStatement<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self
            .entry
            .as_mut()
            .expect("statement remains owned until drop")
            .statement
    }
}

impl ReusedStatement<'_, '_> {
    fn finish<T>(&mut self, result: rusqlite::Result<T>) -> rusqlite::Result<T> {
        self.clear_bindings();
        let memory = self.get_status(StatementStatus::MemUsed) as usize;
        let entry = self.entry.as_mut().expect("active statement is owned");
        let bytes = memory + entry.sql.capacity();
        self.pool.bytes.set(
            self.pool
                .bytes
                .get()
                .saturating_sub(entry.bytes)
                .saturating_add(bytes),
        );
        entry.bytes = bytes;
        if self.pool.bytes.get() > self.pool.limit {
            let mut idle = self.pool.idle.borrow_mut();
            while self.pool.bytes.get() > self.pool.limit && !idle.is_empty() {
                let old = idle.remove(0);
                self.pool
                    .bytes
                    .set(self.pool.bytes.get().saturating_sub(old.bytes));
                self.pool
                    .idle_bytes
                    .set(self.pool.idle_bytes.get().saturating_sub(old.bytes));
            }
        }
        if self.pool.bytes.get() > self.pool.limit {
            return result.and_then(|_| Err(too_large()));
        }
        result
    }
}

impl Drop for ReusedStatement<'_, '_> {
    fn drop(&mut self) {
        let Some(mut entry) = self.entry.take() else {
            return;
        };
        entry.statement.clear_bindings();
        let bytes =
            entry.statement.get_status(StatementStatus::MemUsed) as usize + entry.sql.capacity();
        let total = self.pool.bytes.get().saturating_sub(entry.bytes);
        let mut idle = self.pool.idle.borrow_mut();
        let idle_limit = (self.pool.limit / 4).min(1024 * 1024);
        if total.saturating_add(bytes) > self.pool.limit || bytes > idle_limit {
            self.pool.bytes.set(total);
            return;
        }
        entry.bytes = bytes;
        self.pool.bytes.set(total + bytes);
        while !idle.is_empty()
            && (idle.len() >= 64 || self.pool.idle_bytes.get().saturating_add(bytes) > idle_limit)
        {
            let old = idle.remove(0);
            self.pool
                .bytes
                .set(self.pool.bytes.get().saturating_sub(old.bytes));
            self.pool
                .idle_bytes
                .set(self.pool.idle_bytes.get().saturating_sub(old.bytes));
        }
        self.pool.idle_bytes.set(self.pool.idle_bytes.get() + bytes);
        idle.push(entry);
    }
}

fn too_large() -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_TOOBIG),
        Some("prepared lookups exceed their working memory budget".into()),
    )
}
