// Purpose: 전체 해석의 정렬된 의존성을 페이지별 한 번의 쓰기로 공개.
// Pattern: Bounded Bulk Writer.
// Usage: dependency fresh publication이 파일·사전 그룹 순으로 행을 전달.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/publication.rs, crates/kodstar-core/src/infrastructure/index/dependencies/pages/mutation.rs.

use super::{Row, codec, mutation};
use rusqlite::{Connection, Result};

pub(in crate::infrastructure::index::dependencies) struct Writer<'a> {
    connection: &'a Connection,
    rows: Vec<Row>,
    group: i64,
    file: i64,
    bytes: usize,
}

impl<'a> Writer<'a> {
    pub fn new(connection: &'a Connection) -> Self {
        Self {
            connection,
            rows: Vec::new(),
            group: 0,
            file: 0,
            bytes: 0,
        }
    }
    pub fn push(&mut self, file: i64, mut row: Row) -> Result<()> {
        let group = (row.key - 1) / 128;
        if self.file != file
            || self.group != group
            || self.rows.len() == codec::SLOTS
            || self.bytes + row.payload.len() + 40 > codec::MAX_BYTES
        {
            self.flush()?;
        }
        self.group = group;
        self.file = file;
        row.slot = self.rows.len();
        self.bytes += row.payload.len() + 40;
        self.rows.push(row);
        Ok(())
    }
    pub fn flush(&mut self) -> Result<()> {
        if !self.rows.is_empty() {
            mutation::store(self.connection, 0, self.group, self.file, &self.rows)?;
            self.rows.clear();
            self.bytes = 0;
        }
        Ok(())
    }
}
