// Purpose: Batch final expression dependencies without delaying lookup-visible scope evidence.
// Pattern: Bounded Write Batch.
// Usage: Prepared records expression results and flushes before dependency publication.
// Related: crates/kodstar-core/src/application/analysis/resolve/prepared.rs, crates/kodstar-core/src/application/analysis/resolve/prepared/dependencies.sql.

use crate::domain::diagnostics::Result;
use rusqlite::{Connection, ToSql, params, types::Null};

const SINGLE: &str = "INSERT OR REPLACE INTO resolution_dependencies VALUES(?1,'expression',?2,?3)";

struct Row {
    fact: i64,
    key: String,
    found: bool,
}

pub(super) struct Journal<'conn> {
    connection: &'conn Connection,
    pending: Vec<Row>,
    bytes: usize,
    pub limit: usize,
    pub rows: u64,
    pub statements: u64,
    pub steps: u64,
}

impl<'conn> Journal<'conn> {
    pub fn new(connection: &'conn Connection, limit: usize) -> Self {
        Self {
            connection,
            pending: Vec::new(),
            bytes: 0,
            limit: (limit / 64).min(32768),
            rows: 0,
            statements: 0,
            steps: 0,
        }
    }

    pub fn record(&mut self, fact: i64, key: String, found: bool) -> Result<()> {
        if let Some(row) = self
            .pending
            .iter_mut()
            .find(|row| row.fact == fact && row.key == key)
        {
            row.found = found;
            return Ok(());
        }
        let overhead = 32 * (std::mem::size_of::<Row>() + 3 * std::mem::size_of::<&dyn ToSql>());
        let bytes = key.capacity();
        if bytes.saturating_add(overhead) > self.limit {
            self.flush()?;
            let mut statement = self.connection.prepare_cached(SINGLE)?;
            let before = statement.get_status(rusqlite::StatementStatus::VmStep);
            statement.execute(params![fact, key, found])?;
            self.steps += (statement.get_status(rusqlite::StatementStatus::VmStep) - before) as u64;
            self.rows += 1;
            self.statements += 1;
            return Ok(());
        }
        if self.pending.len() == 32 || self.bytes + bytes + overhead > self.limit {
            self.flush()?;
        }
        self.pending.push(Row { fact, key, found });
        self.bytes += bytes;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let null = Null;
        let mut parameters: Vec<&dyn ToSql> = Vec::with_capacity(96);
        for i in 0..32 {
            if let Some(row) = self.pending.get(i) {
                parameters.extend([&row.fact as &dyn ToSql, &row.key, &row.found]);
            } else {
                parameters.extend([&null as &dyn ToSql; 3]);
            }
        }
        let mut statement = self
            .connection
            .prepare_cached(include_str!("dependencies.sql"))?;
        let before = statement.get_status(rusqlite::StatementStatus::VmStep);
        statement.execute(parameters.as_slice())?;
        self.steps += (statement.get_status(rusqlite::StatementStatus::VmStep) - before) as u64;
        self.rows += self.pending.len() as u64;
        self.statements += 1;
        self.pending.clear();
        self.bytes = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_dependencies_preserve_replacement_order_and_rollback() -> Result<()> {
        let c = Connection::open_in_memory()?;
        c.execute_batch("CREATE TABLE resolution_dependencies(fact_id INTEGER CHECK(fact_id>0),kind TEXT,lookup_key TEXT,found INTEGER,PRIMARY KEY(fact_id,kind,lookup_key));BEGIN;")?;
        let mut journal = Journal::new(&c, 1024 * 1024);
        journal.record(1, "key".into(), true)?;
        journal.record(2, "missing".into(), false)?;
        journal.record(1, "key".into(), false)?;
        journal.flush()?;
        assert_eq!(journal.statements, 1);
        let count: i64 = c.query_row(
            "SELECT count(*) FROM resolution_dependencies WHERE found=0",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 2);
        journal.record(3, "valid".into(), true)?;
        journal.record(-1, "invalid".into(), true)?;
        assert!(journal.flush().is_err());
        assert_eq!(
            c.query_row("SELECT count(*) FROM resolution_dependencies", [], |r| r
                .get::<_, i64>(0))?,
            2
        );
        c.execute_batch("ROLLBACK")?;
        assert_eq!(
            c.query_row("SELECT count(*) FROM resolution_dependencies", [], |r| r
                .get::<_, i64>(0))?,
            0
        );
        Ok(())
    }
}
