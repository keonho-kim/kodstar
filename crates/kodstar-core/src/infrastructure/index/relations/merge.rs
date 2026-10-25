// Purpose: 부분 공개가 기존 source 블록의 빈 자리를 재사용해 블록 수 증가를 제한.
// Pattern: Bounded Merge.
// Usage: publication이 정렬된 새 관계를 전달하고 마지막 배치를 완료.
// Related: crates/kodstar-core/src/infrastructure/index/relations/publication.rs, crates/kodstar-core/src/infrastructure/index/relations/writer.rs.

use super::{Relation, Writer};
use rusqlite::{Connection, OptionalExtension, Result, Statement, params};

pub(super) struct Blocks<'a> {
    connection: &'a Connection,
    writer: Writer<'a>,
    next: Statement<'a>,
    limit: i64,
    group: Option<(i64, i64)>,
    after: i64,
    block: i64,
    rows: Vec<Relation>,
    bytes: usize,
    dirty: bool,
}

impl<'a> Blocks<'a> {
    pub fn new(c: &'a Connection) -> Result<Self> {
        Ok(Self {
            connection: c, writer: Writer::new(c)?,
            next: c.prepare("SELECT id FROM relation_blocks WHERE file_id=?1 AND source_id=?2 AND id>?3 AND id<=?4 AND count<128 ORDER BY id LIMIT 1")?,
            limit: c.query_row("SELECT coalesce(max(id),0) FROM relation_blocks", [], |r| r.get(0))?,
            group: None, after: 0, block: 0, rows: Vec::with_capacity(128), bytes: 0, dirty: false,
        })
    }

    pub fn push(&mut self, file: i64, source: i64, row: Relation) -> Result<()> {
        if self.group != Some((file, source)) {
            self.flush()?;
            self.group = Some((file, source));
            self.after = 0;
            self.load_next(file, source)?;
        }
        let bytes = row.owned_bytes();
        while !self.rows.is_empty() && (self.rows.len() == 128 || self.bytes + bytes > 65536) {
            self.flush()?;
            self.load_next(file, source)?;
        }
        self.bytes += bytes;
        self.rows.push(row);
        self.dirty = true;
        Ok(())
    }

    fn load_next(&mut self, file: i64, source: i64) -> Result<()> {
        self.rows.clear();
        self.bytes = 0;
        self.block = 0;
        if self.after < self.limit {
            let next = self
                .next
                .query_row(params![file, source, self.after, self.limit], |r| {
                    r.get::<_, i64>(0)
                })
                .optional()?;
            if let Some(block) = next {
                self.after = block;
                self.block = block;
                self.rows = super::load(self.connection, block)?.2;
                self.bytes = self.rows.iter().map(Relation::owned_bytes).sum();
            } else {
                self.after = self.limit;
            }
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        if self.dirty {
            let (file, source) = self.group.ok_or_else(super::invalid)?;
            self.rows.sort_unstable_by_key(|row| row.id);
            self.writer.store(self.block, file, source, &self.rows)?;
            self.dirty = false;
        }
        Ok(())
    }
}
