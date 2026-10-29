// Purpose: 블록 공개 중 준비된 SQL과 숫자 검색 경로 배치를 재사용.
// Pattern: Single Writer and Bounded Batch.
// Usage: 관계 publication·부분 무효화가 한 transaction에서 소유.
// Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs, crates/kodstar-core/src/infrastructure/index/relations/publication.rs.

use super::{Relation, codec};
use rusqlite::{Connection, Result, Statement, params};
use serde::{Serialize, Serializer, ser::SerializeSeq};

const ROUTE_ENTITIES: &str = "SELECT 1 AS kind,json_extract(value,'$[1]') AS entity FROM jsonb_each(?1)
    UNION ALL SELECT 3,json_extract(value,'$[2]') FROM jsonb_each(?1) WHERE json_extract(value,'$[2]') IS NOT NULL
    UNION ALL SELECT 6,json_extract(value,'$[3]') FROM jsonb_each(?1)
    UNION ALL SELECT 6,json_extract(value,'$[4]') FROM jsonb_each(?1) WHERE json_extract(value,'$[4]') IS NOT NULL";

pub(super) struct Writer<'a> {
    connection: &'a Connection,
    pack: Statement<'a>,
    remove_ids: Statement<'a>,
    remove_routes: Statement<'a>,
    remove_block: Statement<'a>,
    block: Statement<'a>,
    ids: Statement<'a>,
    routes: Statement<'a>,
}

impl<'a> Writer<'a> {
    pub fn new(c: &'a Connection) -> Result<Self> {
        Ok(Self {
            connection: c,
            pack: c.prepare("SELECT jsonb(?1)")?,
            remove_ids: c.prepare(&format!("DELETE FROM relation_id_ranges WHERE block=?2 AND (first,last) NOT IN ({})", super::addresses::RANGES))?,
            remove_routes: c.prepare(&format!("DELETE FROM relation_routes WHERE block=?2 AND (kind,entity) NOT IN ({ROUTE_ENTITIES})"))?,
            remove_block: c.prepare("DELETE FROM relation_blocks WHERE id=?1")?,
            block: c.prepare("INSERT INTO relation_blocks(id,file_id,source_id,count,payload) VALUES(nullif(?1,0),?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET count=excluded.count,payload=excluded.payload WHERE relation_blocks.count<>excluded.count OR relation_blocks.payload<>excluded.payload")?,
            ids: c.prepare(&format!("INSERT OR IGNORE INTO relation_id_ranges SELECT first,last,?2 FROM ({})", super::addresses::RANGES))?,
            routes: c.prepare(&format!("INSERT OR IGNORE INTO relation_routes SELECT kind,entity,?2 FROM ({ROUTE_ENTITIES})"))?,
        })
    }

    pub fn store(&mut self, block: i64, file: i64, source: i64, rows: &[Relation]) -> Result<i64> {
        let routes = serde_json::to_string(&Routes(rows))
            .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
        let packed = self.pack.query_row([&routes], |r| r.get::<_, Vec<u8>>(0));
        self.pack.clear_bindings();
        let routes = packed?;
        if block != 0 {
            self.remove_ids.execute(params![routes, block])?;
            self.remove_routes.execute(params![routes, block])?;
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
            codec::encode(rows)?
        ])?;
        let block = if block == 0 {
            self.connection.last_insert_rowid()
        } else {
            block
        };
        // Retain equal mappings and routes; only changed entries reach SQLite's writer.
        self.ids.execute(params![routes, block])?;
        self.routes.execute(params![routes, block])?;
        Ok(block)
    }
}

struct Routes<'a>(&'a [Relation]);
impl Serialize for Routes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for row in self.0 {
            sequence.serialize_element(&row.routes())?;
        }
        sequence.end()
    }
}
