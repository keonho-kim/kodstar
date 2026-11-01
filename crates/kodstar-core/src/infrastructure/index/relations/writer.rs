// Purpose: 블록 공개 중 준비된 SQL과 숫자 검색 경로 배치를 재사용.
// Pattern: Single Writer and Bounded Batch.
// Usage: 관계 publication·부분 무효화가 한 transaction에서 소유.
// Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs, crates/kodstar-core/src/infrastructure/index/relations/publication.rs.

use super::{Relation, codec, routing, sets};
use rusqlite::{Connection, Result, Statement, params};
use serde::{Serialize, Serializer, ser::SerializeSeq};

pub(super) struct Writer<'a> {
    connection: &'a Connection,
    pack: Statement<'a>,
    remove_ids: Statement<'a>,
    remove_block: Statement<'a>,
    block: Statement<'a>,
    ids: Statement<'a>,
    sets: sets::Writer<'a>,
}

impl<'a> Writer<'a> {
    pub fn new(c: &'a Connection) -> Result<Self> {
        Ok(Self {
            connection: c,
            pack: c.prepare("SELECT jsonb(?1)")?,
            remove_ids: c.prepare(&format!("DELETE FROM relation_id_ranges WHERE block=?2 AND (first,last) NOT IN ({})", super::addresses::RANGES))?,
            remove_block: c.prepare("DELETE FROM relation_blocks WHERE id=?1")?,
            block: c.prepare("INSERT INTO relation_blocks(id,file_id,source_id,count,payload,routes) VALUES(nullif(?1,0),?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET count=excluded.count,payload=excluded.payload,routes=excluded.routes WHERE relation_blocks.count<>excluded.count OR relation_blocks.payload<>excluded.payload OR relation_blocks.routes<>excluded.routes")?,
            ids: c.prepare(&format!("INSERT OR IGNORE INTO relation_id_ranges SELECT first,last,?2 FROM ({})", super::addresses::RANGES))?,
            sets: sets::Writer::new(c)?,
        })
    }

    pub fn store(&mut self, block: i64, file: i64, source: i64, rows: &[Relation]) -> Result<i64> {
        let routes = serde_json::to_string(&Ids(rows))
            .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
        let packed = self.pack.query_row([&routes], |r| r.get::<_, Vec<u8>>(0));
        self.pack.clear_bindings();
        let routes = packed?;
        if block != 0 {
            self.remove_ids.execute(params![routes, block])?;
        }
        if rows.is_empty() {
            self.remove_block.execute([block])?;
            return Ok(block);
        }
        self.block.execute(params![
            block,
            file,
            source,
            rows.len(),
            codec::encode(rows, &mut self.sets)?,
            routing::encode(rows)?
        ])?;
        let block = if block == 0 {
            self.connection.last_insert_rowid()
        } else {
            block
        };
        // Retain equal mappings and routes; only changed entries reach SQLite's writer.
        self.ids.execute(params![routes, block])?;
        Ok(block)
    }
}

struct Ids<'a>(&'a [Relation]);
impl Serialize for Ids<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for row in self.0 {
            sequence.serialize_element(&[row.id])?;
        }
        sequence.end()
    }
}
