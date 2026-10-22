// Purpose: SQLite의 압축 블록 순회를 한 블록 메모리로 제한.
// Pattern: Streaming Cursor.
// Usage: packed virtual table이 source·target·fact·key 조건을 전달.
// Related: crates/kodstar-core/src/infrastructure/index/packed/mod.rs.

use super::{Filter, Kind, Row, codec};
use rusqlite::{Connection, Result, ffi, types::Value, vtab::*};
use std::rc::Rc;

#[repr(C)]
pub(super) struct Cursor {
    base: ffi::sqlite3_vtab_cursor,
    connection: Rc<Connection>,
    kind: Kind,
    filters: Vec<Filter>,
    block: i64,
    rows: Vec<Row>,
    at: usize,
}

impl Cursor {
    pub fn new(connection: Rc<Connection>, kind: Kind) -> Self {
        Self {
            base: ffi::sqlite3_vtab_cursor::default(),
            connection,
            kind,
            filters: Vec::new(),
            block: 0,
            rows: Vec::new(),
            at: 0,
        }
    }

    fn advance(&mut self) -> Result<()> {
        loop {
            while self.at < self.rows.len() {
                let row = &self.rows[self.at];
                let matches =
                    self.filters
                        .iter()
                        .all(|(column, value)| match (&row[*column], value) {
                            (_, Value::Null) => false,
                            (Value::Integer(a), Value::Integer(b)) => a == b,
                            (Value::Text(a), Value::Text(b)) => a == b,
                            // SQLite performs numeric affinity/coercion checks for other types.
                            _ => true,
                        });
                if matches {
                    return Ok(());
                }
                self.at += 1;
            }
            self.rows.clear();
            let next = match self.kind {
                Kind::Dictionary => {
                    super::super::dictionary::scan(&self.connection, self.block, &self.filters)?
                }
                Kind::Relations => {
                    super::super::relations::scan(&self.connection, self.block, &self.filters)?
                }
            };
            let Some((block, rows)) = next else {
                return Ok(());
            };
            if block <= self.block {
                return Err(codec::invalid());
            }
            self.block = block;
            self.rows = rows;
            self.at = 0;
        }
    }
}

// SAFETY: repr(C) with sqlite3_vtab_cursor first; owned Rc/rows outlive callbacks,
// no statements or references to a mutable virtual table survive a callback.
unsafe impl VTabCursor for Cursor {
    fn filter(&mut self, _: i32, plan: Option<&str>, args: &Filters<'_>) -> Result<()> {
        self.filters.clear();
        if let Some(plan) = plan.filter(|p| !p.is_empty()) {
            for (at, column) in plan.split(',').enumerate() {
                self.filters.push((
                    column.parse().map_err(|_| codec::invalid())?,
                    args.get::<Value>(at)?,
                ));
            }
        }
        self.block = 0;
        self.rows.clear();
        self.at = 0;
        self.advance()
    }
    fn next(&mut self) -> Result<()> {
        self.at += 1;
        self.advance()
    }
    fn eof(&self) -> bool {
        self.at >= self.rows.len()
    }
    fn column(&self, context: &mut Context, column: i32) -> Result<()> {
        context.set_result(&self.rows[self.at][column as usize])
    }
    fn rowid(&self) -> Result<i64> {
        match self.rows[self.at][0] {
            Value::Integer(id) => Ok(id),
            _ => Err(codec::invalid()),
        }
    }
}
