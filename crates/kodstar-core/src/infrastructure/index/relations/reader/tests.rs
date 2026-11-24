// Purpose: 블록 읽기의 손실 없는 결과·바이트 한도·rollback 재사용 경계 검증.
// Pattern: Storage Contract Test.
// Usage: relation Reader 단위 테스트에서 실행.
// Related: crates/kodstar-core/src/infrastructure/index/relations/reader.rs.

use super::*;
use crate::infrastructure::index::{
    packed,
    relations::{self, codec},
};

fn fixture() -> Result<Connection> {
    let c = Connection::open_in_memory()?;
    relations::tests::parents(&c)?;
    packed::register(&c)?;
    c.execute_batch(relations::SCHEMA)?;
    c.execute_batch(
        "INSERT INTO files_rows VALUES(1);INSERT INTO symbols_rows VALUES(1),(2),(3);
        INSERT INTO facts_rows VALUES(1,1),(2,1);INSERT INTO relation_texts VALUES(1,'rule');
        INSERT INTO relationships VALUES(1,1,1,2,NULL,'resolved','rule',NULL);
        INSERT INTO relationships VALUES(2,2,1,NULL,'外部','resolved','rule','reason');",
    )?;
    Ok(c)
}

#[test]
fn optimization_block_eviction_keeps_other_blocks_and_texts() -> Result<()> {
    let c = fixture()?;
    for source in 2..=6 {
        c.execute("INSERT OR IGNORE INTO symbols_rows VALUES(?1)", [source])?;
        c.execute("INSERT INTO facts_rows VALUES(?1,1)", [source + 2])?;
        c.execute(
            "INSERT INTO relationships VALUES(?1,?1,?2,2,NULL,'resolved','rule',NULL)",
            rusqlite::params![source + 2, source],
        )?;
    }
    c.execute_batch("BEGIN;SELECT * FROM files_rows;")?;
    let mut reader = Reader::new(&c)?;
    for id in [1, 4, 5, 6] {
        reader.read(id)?;
    }
    reader.text(1)?;
    reader.read(1)?;
    reader.read(7)?;
    assert_eq!(reader.blocks.len(), 4);
    assert_eq!(
        reader.texts.len(),
        1,
        "unrelated text was discarded with a block"
    );
    Ok(())
}

#[test]
fn reads_are_lossless_and_only_committed_read_snapshots_retain_values() -> Result<()> {
    let c = fixture()?;
    c.execute_batch("BEGIN;SELECT * FROM files_rows;")?;
    let mut reader = Reader::new(&c)?;
    assert_eq!(reader.read(1)?.target, Some(2));
    assert_eq!(reader.read(2)?.external.as_deref(), Some("外部"));
    assert_eq!(reader.blocks.len(), 1);
    assert_eq!(reader.text(1)?, "rule");
    c.execute_batch("SAVEPOINT edit;UPDATE relationship_rows SET target_id=3 WHERE id=1;UPDATE relation_texts SET value='edited' WHERE id=1;")?;
    assert_eq!(reader.read(1)?.target, Some(3));
    assert_eq!(reader.text(1)?, "edited");
    assert!(reader.blocks.is_empty() && reader.texts.is_empty());
    c.execute_batch("ROLLBACK TO edit;RELEASE edit;")?;
    assert_eq!(reader.read(1)?.target, Some(2));
    assert_eq!(reader.text(1)?, "rule");
    c.execute_batch("ROLLBACK")?;
    assert_eq!(reader.read(2)?.external.as_deref(), Some("外部"));
    assert!(reader.blocks.is_empty());
    assert!(reader.read(999).is_err());
    Ok(())
}

#[test]
fn column_projection_skips_string_ownership_but_still_checks_the_whole_block() -> Result<()> {
    let c = fixture()?;
    let (source, count, bytes): (i64, usize, Vec<u8>) = c.query_row(
        "SELECT source_id,count,payload FROM relation_blocks",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let rows = codec::project(&c, &bytes, count, source, &[], 1)?;
    assert!(rows.iter().all(|row| row.external.is_none()));
    let rows = codec::project(&c, &bytes, count, source, &[], 1 << 4)?;
    assert_eq!(rows[1].external.as_deref(), Some("外部"));
    assert!(codec::project(&c, &bytes[..bytes.len() - 1], count, source, &[], 1).is_err());
    c.execute_batch("BEGIN;SELECT * FROM files_rows;")?;
    let mut reader = Reader::new(&c)?;
    reader.limit = 1;
    reader.read(2)?;
    assert!(reader.blocks.is_empty());
    c.execute_batch("ROLLBACK")?;
    drop(reader);
    c.close().map_err(|(_, error)| error)?;
    Ok(())
}

#[test]
fn oversized_shared_groups_only_copy_the_requested_external_value() -> Result<()> {
    let c = fixture()?;
    let rows = (1..=128)
        .map(|id| Relation {
            id,
            fact: 1,
            source: 1,
            target: None,
            external: Some("x".repeat(2048)),
            state: crate::domain::relations::Resolution::Resolved,
            rule: 1,
            reason: None,
        })
        .collect::<Vec<_>>();
    let mut sets = relations::sets::Writer::new(&c)?;
    let bytes = codec::encode(&rows, &mut sets)?;
    let (selected, retained, _) = read_block(&c, &bytes, 128, 1, 128, 20 * 1024)?;
    assert!(retained.is_none());
    assert_eq!(selected, rows[127]);
    Ok(())
}
