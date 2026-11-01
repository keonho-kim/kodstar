// Purpose: 역조회 페이지 경계·64비트 ID·FK 삭제·손상 검증.
// Pattern: Storage Regression Test.
// Usage: core 단위 검사가 codec과 실제 관계 cursor를 검증.
// Related: crates/kodstar-core/src/infrastructure/index/relations/routing/mod.rs, crates/kodstar-core/src/infrastructure/index/relations/verification.rs.

use super::*;
use crate::domain::relations::Resolution;
use rusqlite::types::Value;

#[test]
fn pages_cover_boundary_and_large_block_ids_without_losing_cascades() -> Result<()> {
    let c = Connection::open_in_memory()?;
    super::super::tests::parents(&c)?;
    crate::infrastructure::index::packed::register(&c)?;
    c.execute_batch(super::super::SCHEMA)?;
    c.execute_batch(
        "PRAGMA foreign_keys=ON; INSERT INTO files_rows VALUES(1);
        INSERT INTO symbols_rows VALUES(1),(2); INSERT INTO facts_rows VALUES(1,1);
        INSERT INTO relation_texts VALUES(1,'fixture');",
    )?;
    let blocks = [1, 1024, 1025, 2048, i64::MAX];
    let mut writer = super::super::Writer::new(&c)?;
    for (n, block) in blocks.into_iter().enumerate() {
        writer.store(
            block,
            1,
            1,
            &[Relation {
                id: n as i64 + 1,
                fact: 1,
                source: 1,
                target: Some(2),
                external: None,
                state: Resolution::Resolved,
                rule: 1,
                reason: None,
            }],
        )?;
    }
    let mut cursor = Cursor::default();
    let mut after = 0;
    for expected in blocks {
        assert_eq!(
            cursor.next(&c, 3, &Value::Integer(2), after)?,
            Some(expected)
        );
        after = expected;
    }
    assert_eq!(cursor.next(&c, 3, &Value::Integer(2), after)?, None);
    super::super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    c.execute_batch("BEGIN; UPDATE relation_route_pages SET payload=X'00';")?;
    assert!(super::super::verify(&c).is_err());
    c.execute_batch("ROLLBACK;")?;
    let mut live = Cursor::default();
    assert_eq!(live.next(&c, 3, &Value::Integer(2), 0)?, Some(1));
    c.execute("DELETE FROM facts_rows WHERE id=1", [])?;
    assert_eq!(live.next(&c, 3, &Value::Integer(2), 1)?, None);
    assert_eq!(
        c.query_row("SELECT count(*) FROM relation_route_pages", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    Ok(())
}

#[test]
fn page_edits_are_idempotent_and_reject_malformed_or_unbounded_slots() -> Result<()> {
    let bytes = codec::change(&[], 0, true)?;
    assert_eq!(codec::change(&bytes, 0, true)?, bytes);
    let bytes = codec::change(&bytes, 1023, true)?;
    assert_eq!(codec::decode(&bytes)?, vec![0, 1023]);
    assert_eq!(codec::change(&bytes, 500, false)?, bytes);
    assert!(codec::change(&bytes, 1024, true).is_err());
    for malformed in [&[0][..], &[128, 0][..], &[129, 8][..]] {
        assert!(codec::decode(malformed).is_err());
    }
    assert!(codec::absolute(i64::MAX, 0).is_err());
    Ok(())
}

#[test]
fn updating_one_block_does_not_scan_unrelated_route_pages() -> Result<()> {
    let c = Connection::open_in_memory()?;
    super::super::tests::parents(&c)?;
    crate::infrastructure::index::packed::register(&c)?;
    c.execute_batch(super::super::SCHEMA)?;
    c.execute_batch(
        "PRAGMA foreign_keys=ON; INSERT INTO files_rows VALUES(1);
        INSERT INTO symbols_rows VALUES(1),(2); INSERT INTO relation_texts VALUES(1,'fixture');
        WITH RECURSIVE ids(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM ids WHERE n<257)
        INSERT INTO facts_rows SELECT n,1 FROM ids;",
    )?;
    let row = |id| Relation {
        id,
        fact: id,
        source: 1,
        target: Some(2),
        external: None,
        state: Resolution::Resolved,
        rule: 1,
        reason: None,
    };
    let mut writer = super::super::Writer::new(&c)?;
    for id in 1..=256 {
        writer.store(id, 1, 1, &[row(id)])?;
    }
    let rows = [row(1), row(257)];
    let mut sets = super::super::sets::Writer::new(&c)?;
    let payload = super::super::codec::encode(&rows, &mut sets)?;
    let routes = super::encode(&rows)?;
    let mut update =
        c.prepare("UPDATE relation_blocks SET count=2,payload=?1,routes=?2 WHERE id=1")?;
    update.execute(rusqlite::params![payload, routes])?;
    let scanned = update.get_status(rusqlite::StatementStatus::FullscanStep);
    assert!(
        scanned < 50,
        "one block scanned {scanned} unrelated route rows"
    );
    let mut delete = c.prepare("DELETE FROM relation_blocks WHERE id=1")?;
    c.execute("INSERT INTO relation_id_ranges VALUES(257,257,1)", [])?;
    super::super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    delete.execute([])?;
    let scanned = delete.get_status(rusqlite::StatementStatus::FullscanStep);
    assert!(
        scanned < 50,
        "one block deletion scanned {scanned} unrelated route rows"
    );
    super::super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    Ok(())
}
