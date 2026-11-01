// Purpose: 경로별 변화 합성이 분기·되돌림·삭제·속성과 전체 재구성에 일치하는지 검증.
// Pattern: Differential Storage Test.
// Usage: history changes의 계보 경로와 전체 대조 경로를 함께 검사.
// Related: crates/kodstar-core/src/infrastructure/history/changes/mod.rs.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn fixture() -> Result<Connection> {
    let c = Connection::open_in_memory()?;
    c.execute_batch(include_str!("../schema.sql"))?;
    c.execute_batch(include_str!("../metadata.sql"))?;
    c.execute_batch(
        "INSERT INTO blobs VALUES(1,zeroblob(32),0),(2,randomblob(32),0),(3,randomblob(32),0);
        INSERT INTO checkpoints VALUES(1,NULL,0,1000,1000),(2,1,0,1,1000),
          (3,2,0,1,1000),(4,1,0,1,999),(5,4,0,1,1000);
        UPDATE branches SET checkpoint=3;
        WITH RECURSIVE ids(i) AS(VALUES(1) UNION ALL SELECT i+1 FROM ids WHERE i<1000)
        INSERT INTO paths SELECT i,CAST('file-'||i AS BLOB) FROM ids;
        INSERT INTO entry_rows SELECT 1,id,1,420,NULL,NULL FROM paths;
        INSERT INTO entry_rows VALUES(2,1,2,420,1,420),(3,1,1,420,2,420),
          (4,2,NULL,420,1,420),(5,2,3,1005,NULL,NULL);
        INSERT INTO working SELECT value,1,420,'file',0,'','' FROM paths;",
    )?;
    Ok(c)
}

fn state(c: &Connection, id: Option<i64>) -> Result<BTreeMap<Vec<u8>, (i64, i64)>> {
    let Some(id) = id else {
        return Ok(BTreeMap::new());
    };
    Ok(c.prepare("WITH RECURSIVE ancestors(id) AS(SELECT ?1 UNION ALL
        SELECT c.parent FROM checkpoints c JOIN ancestors a ON a.id=c.id WHERE c.parent IS NOT NULL),
        latest AS(SELECT path,max(checkpoint) AS checkpoint FROM entry_rows
            WHERE checkpoint IN (SELECT id FROM ancestors) GROUP BY path)
        SELECT p.value,e.blob,e.attributes FROM entry_rows e JOIN latest l
            ON l.path=e.path AND l.checkpoint=e.checkpoint JOIN paths p ON p.id=e.path
        WHERE e.blob IS NOT NULL")?
        .query_map([id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
        .collect::<rusqlite::Result<_>>()?)
}

#[test]
fn net_changes_match_independent_reconstruction_in_both_directions() -> Result<()> {
    let c = fixture()?;
    for from in [None, Some(1), Some(2), Some(3), Some(4), Some(5)] {
        for to in 1..=5 {
            let before = state(&c, from)?;
            let after = state(&c, Some(to))?;
            let paths: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
            let expected: Vec<_> = paths
                .into_iter()
                .filter_map(|path| {
                    let a = before.get(&path).copied();
                    let b = after.get(&path).copied();
                    (a != b).then_some((path, a, b))
                })
                .collect();
            let count = prepare(&c, from, to, &Cancellation::default())?;
            let actual: Vec<_> = c
                .prepare(
                    "SELECT path,before_blob,before_attributes,after_blob,after_attributes
                FROM snapshot_changes ORDER BY path",
                )?
                .query_map([], |r| {
                    let a: Option<i64> = r.get(1)?;
                    let b: Option<i64> = r.get(3)?;
                    Ok((r.get::<_, Vec<u8>>(0)?, a.zip(r.get(2)?), b.zip(r.get(4)?)))
                })?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(actual, expected, "{from:?} -> {to}");
            assert_eq!(count as usize, expected.len());
        }
    }
    Ok(())
}

#[test]
fn reverted_changes_do_not_materialize_a_thousand_file_catalog() -> Result<()> {
    let c = fixture()?;
    assert_eq!(prepare(&c, Some(1), 3, &Cancellation::default())?, 0);
    assert!(!c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_schema
        WHERE name IN ('snapshot_files','change_route','change_before'))",
        [],
        |r| r.get::<_, bool>(0)
    )?);
    Ok(())
}

#[test]
fn expensive_history_uses_full_comparison_and_cancellation_cleans_its_route() -> Result<()> {
    let c = fixture()?;
    c.execute_batch("UPDATE checkpoints SET changed_files=100000 WHERE id=2;")?;
    assert_eq!(prepare(&c, Some(2), 5, &Cancellation::default())?, 2);
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(prepare(&c, Some(2), 3, &cancel).is_err());
    assert!(!c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_schema WHERE name='change_route')",
        [],
        |r| r.get::<_, bool>(0)
    )?);
    assert_eq!(prepare(&c, Some(1), 3, &Cancellation::default())?, 0);
    Ok(())
}
