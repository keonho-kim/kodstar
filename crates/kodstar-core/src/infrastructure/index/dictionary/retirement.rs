// Purpose: 더 이상 소비되지 않는 사전 key를 블록 단위로 회수.
// Pattern: Bounded Garbage Collection.
// Usage: dependency publication 완료 뒤 같은 세대 transaction에서 실행.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/mod.rs.

use rusqlite::{Connection, OptionalExtension, Result};

pub(crate) fn run(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE dictionary_retired(id INTEGER PRIMARY KEY,block INTEGER);
        INSERT INTO dictionary_retired SELECT d.id,kodstar_route_at(p.payload,(d.id-1)%128) FROM dictionary_retirements d JOIN dictionary_routes p ON p.id=(d.id-1)/128 WHERE NOT EXISTS(SELECT 1 FROM lookup_blocks b WHERE b.key_id=d.id);
        CREATE INDEX dictionary_retired_block ON dictionary_retired(block);")?;
    let mut after = 0;
    loop {
        let block: Option<i64> = c
            .query_row(
                "SELECT block FROM dictionary_retired WHERE block>?1 ORDER BY block LIMIT 1",
                [after],
                |r| r.get(0),
            )
            .optional()?;
        let Some(block) = block else {
            break;
        };
        let (kind, count, bytes): (String, usize, Vec<u8>) = c.query_row(
            "SELECT kind,count,payload FROM dictionary_blocks WHERE id=?1",
            [block],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut retained = Vec::new();
        let mut contains =
            c.prepare("SELECT EXISTS(SELECT 1 FROM dictionary_retired WHERE id=?1)")?;
        for (id, key) in super::codec::decode(&bytes, count)? {
            if !contains.query_row([id], |r| r.get::<_, bool>(0))? {
                retained.push((id, key));
            }
        }
        drop(contains);
        if retained.is_empty() {
            let mut old = Vec::new();
            super::codec::select(&bytes, count, |id, _| {
                old.push(id);
                false
            })?;
            super::routing::update(c, block, &old, &[])?;
            c.execute("DELETE FROM dictionary_blocks WHERE id=?1", [block])?;
        } else {
            super::store(c, block, &kind, &retained)?;
        }
        after = block;
    }
    c.execute_batch("DROP TABLE dictionary_retired; DELETE FROM dictionary_retirements;")
}
