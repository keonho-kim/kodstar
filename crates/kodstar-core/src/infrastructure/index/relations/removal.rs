// Purpose: 부분 무효화에서 관계 블록을 한 번씩만 다시 작성.
// Pattern: Bounded Mutation.
// Usage: incremental resolver가 삭제할 논리 관계를 먼저 디스크에 선택.
// Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs, crates/kodstar-core/src/infrastructure/index/invalidation.rs.

use rusqlite::{Connection, Result, types::Value};

pub(crate) fn partial(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE relation_pending_blocks(id INTEGER PRIMARY KEY);
        INSERT INTO relation_pending_blocks SELECT id FROM relation_blocks WHERE file_id IN (SELECT id FROM changed_files);")?;
    let mut after = 0;
    loop {
        use rusqlite::OptionalExtension;
        let block: Option<i64> = c
            .query_row(
                "SELECT id FROM relation_pending_blocks WHERE id>?1 ORDER BY id LIMIT 1",
                [after],
                |r| r.get(0),
            )
            .optional()?;
        let Some(block) = block else {
            break;
        };
        let (file, source, mut rows) = super::load(c, block)?;
        let mut retained = Vec::with_capacity(rows.len());
        let mut contains =
            c.prepare("SELECT language='go' AND relation='implements' AND expression LIKE '@go-interface:%' FROM facts WHERE id=?1")?;
        for row in rows.drain(..) {
            let Value::Integer(id) = row[1] else {
                return Err(super::invalid());
            };
            if contains.query_row([id], |r| r.get::<_, bool>(0))? {
                retained.push(row);
            }
        }
        drop(contains);
        super::store(c, block, file, source, &retained)?;
        after = block;
    }
    c.execute_batch("DROP TABLE relation_pending_blocks;")
}
