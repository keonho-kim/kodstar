// Purpose: 압축 관계와 ID·역방향 경로·fact·정의·텍스트 참조 검증.
// Pattern: Integrity Check.
// Usage: doctor가 공개된 분석 snapshot의 물리 검색 경로를 대조.
// Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs.

use crate::domain::diagnostics::{Error, Result};
use rusqlite::{Connection, params};

pub(crate) fn verify(c: &Connection) -> Result<()> {
    super::routing::verify(c)?;
    super::sets::verify(c)?;
    let malformed: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM relation_id_ranges
        WHERE first<=0 OR typeof(last)<>'integer' OR last<first OR last-first>=128)",
        [],
        |r| r.get(0),
    )?;
    if malformed {
        return Err(Error::operation("invalid relationship routing interval"));
    }
    c.execute_batch("CREATE TEMP TABLE relation_expected_sets(id INTEGER PRIMARY KEY,refs INTEGER);
        CREATE TEMP TABLE relation_expected_ids(id INTEGER PRIMARY KEY,block INTEGER);
        CREATE TEMP TABLE relation_expected_routes(kind INTEGER,entity INTEGER,block INTEGER,PRIMARY KEY(kind,entity,block)) WITHOUT ROWID;")?;
    let mut select = c.prepare("SELECT id FROM relation_blocks ORDER BY id")?;
    let mut blocks = select.query([])?;
    let mut ids = c.prepare("INSERT INTO relation_expected_ids VALUES(?1,?2)")?;
    let mut routes =
        c.prepare("INSERT OR IGNORE INTO relation_expected_routes VALUES(?1,?2,?3)")?;
    let mut references=c.prepare("SELECT EXISTS(SELECT 1 FROM facts WHERE id=?1 AND file_id=?2) AND EXISTS(SELECT 1 FROM symbols WHERE id=?3) AND (?4 IS NULL OR EXISTS(SELECT 1 FROM symbols WHERE id=?4)) AND EXISTS(SELECT 1 FROM relation_texts WHERE id=?5) AND (?6 IS NULL OR EXISTS(SELECT 1 FROM relation_texts WHERE id=?6))")?;
    while let Some(block) = blocks.next()? {
        let block: i64 = block.get(0)?;
        let (file, source, rows) = super::load(c, block)?;
        let (bytes, stored_routes): (Vec<u8>, Vec<u8>) = c.query_row(
            "SELECT payload,routes FROM relation_blocks WHERE id=?1",
            [block],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if stored_routes != super::routing::encode(&rows)? {
            return Err(Error::operation(
                "relationship block has inconsistent reverse keys",
            ));
        }
        for id in super::groups::references(&bytes, rows.len())? {
            c.execute("INSERT INTO relation_expected_sets VALUES(?1,1) ON CONFLICT(id) DO UPDATE SET refs=refs+1", [id])?;
        }
        for row in rows {
            if !references.query_row(
                params![row.fact, file, source, row.target, row.rule, row.reason],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(Error::operation(
                    "packed relationship references a missing or different owner",
                ));
            }
            ids.execute(params![row.id, block])?;
            routes.execute(params![1, row.fact, block])?;
            routes.execute(params![6, row.rule, block])?;
            if row.reason.is_some() {
                routes.execute(params![6, row.reason, block])?;
            }
            if row.target.is_some() {
                routes.execute(params![3, row.target, block])?;
            }
        }
    }
    drop(blocks);
    drop(select);
    drop(ids);
    drop(routes);
    drop(references);
    let invalid:bool=c.query_row("SELECT EXISTS(SELECT * FROM relation_expected_ids EXCEPT SELECT * FROM relation_ids) OR EXISTS(SELECT * FROM relation_ids EXCEPT SELECT * FROM relation_expected_ids) OR EXISTS(SELECT * FROM relation_expected_routes EXCEPT SELECT * FROM relation_routes) OR EXISTS(SELECT * FROM relation_routes EXCEPT SELECT * FROM relation_expected_routes)
        OR EXISTS(SELECT id,refs FROM relation_sets EXCEPT SELECT * FROM relation_expected_sets)
        OR EXISTS(SELECT * FROM relation_expected_sets EXCEPT SELECT id,refs FROM relation_sets)",[],|r|r.get(0))?;
    c.execute_batch("DROP TABLE relation_expected_ids; DROP TABLE relation_expected_routes; DROP TABLE relation_expected_sets;")?;
    if invalid {
        return Err(Error::operation("packed relationship routing mismatch"));
    }
    Ok(())
}
