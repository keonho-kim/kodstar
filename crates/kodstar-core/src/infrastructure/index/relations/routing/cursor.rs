// Purpose: 한 관계 cursor가 역조회 페이지 하나를 보유하며 block ID를 순회.
// Pattern: Streaming Cursor.
// Usage: packed cursor가 filter마다 초기화하고 같은 selector로 전진.
// Related: crates/kodstar-core/src/infrastructure/index/relations/routing/codec.rs, crates/kodstar-core/src/infrastructure/index/packed/cursor.rs.

use super::codec;
use rusqlite::{Connection, OptionalExtension, Result, params, types::Value};

#[derive(Default)]
pub(in crate::infrastructure::index) struct Cursor {
    page: Option<i64>,
    values: Vec<i64>,
    changes: u64,
}

impl Cursor {
    pub fn next(
        &mut self,
        c: &Connection,
        kind: usize,
        value: &Value,
        after: i64,
    ) -> Result<Option<i64>> {
        // xDelete and FK cascades can remove a future block on this same handle.
        // A rollback also advances total_changes; reread routing after either effect.
        let changes = c.total_changes();
        if self.changes != changes {
            self.page = None;
            self.values.clear();
            self.changes = changes;
        }
        let at = self.values.partition_point(|id| *id <= after);
        if let Some(id) = self.values.get(at) {
            return Ok(Some(*id));
        }
        let start = self.page.map_or(after / codec::WIDTH, |page| {
            (page + 1).max(after / codec::WIDTH)
        });
        let page = c.query_row(
            "SELECT page,payload FROM relation_route_pages WHERE kind=?1 AND entity=?2 AND page>=?3 ORDER BY page LIMIT 1",
            params![kind,value,start], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,Vec<u8>>(1)?))).optional()?;
        let Some((page, bytes)) = page else {
            return Ok(None);
        };
        self.page = Some(page);
        self.values = codec::decode(&bytes)?
            .into_iter()
            .map(|slot| codec::absolute(page, slot))
            .collect::<Result<_>>()?;
        if self.values.is_empty() {
            return Err(crate::infrastructure::index::packed::codec::invalid());
        }
        // The first page may contain only earlier IDs; the next page is strictly later.
        self.next(c, kind, value, after)
    }
}
