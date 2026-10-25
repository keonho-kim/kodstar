// Purpose: 형식 4 전환의 독립·델타·고아 청크 보존과 취소·손상 rollback 검증.
// Pattern: Storage Regression Test.
// Usage: 실제 형식 4 payload를 SQLite에 넣고 명시적 upgrade를 실행.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/compressed.rs.

use super::*;
use crate::infrastructure::history::content::{blobs, chunk_read, chunks};

fn fixture() -> Result<(Connection, Vec<u8>, Vec<u8>)> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(include_str!("../../schema.sql"))?;
    connection.execute_batch(chunks::SCHEMA)?;
    connection.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA user_version=4;
        INSERT INTO checkpoints VALUES(7,NULL,123,2,2);
        UPDATE branches SET checkpoint=7;
        INSERT INTO names VALUES('saved',7);",
    )?;
    let mut random = 42u64;
    let mut base: Vec<u8> = (0..8192)
        .map(|_| {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            random as u8
        })
        .collect();
    base[..4].copy_from_slice(&[0x37, 0xa4, 0x30, 0xec]);
    let mut changed = base.clone();
    changed[8191] ^= 1;
    let repeated = vec![b'a'; 8192];
    for (id, raw, codec, prior, payload) in [
        (11, &base, 0, None, base.clone()),
        (
            22,
            &changed,
            2,
            Some(11),
            vec![0xf7, 0x7f, 0, 0, changed[8191]],
        ),
        (99, &repeated, 1, None, vec![0, b'a', 0xf7, 0x7f, 0]),
    ] {
        let (lines, first, last) = line_info(raw);
        connection.execute(
            "INSERT INTO chunk_data VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                id,
                blake3::hash(raw).as_bytes().as_slice(),
                raw.len(),
                codec,
                prior,
                lines,
                first,
                last,
                payload
            ],
        )?;
        if id != 99 {
            connection.execute(
                "INSERT INTO blobs VALUES(?1,?2,?3)",
                params![id, blake3::hash(raw).to_hex().as_str(), raw.len()],
            )?;
            connection.execute("INSERT INTO chunk_refs VALUES(?1,0,0,?1)", [id])?;
        }
    }
    Ok((connection, base, changed))
}

#[test]
fn upgrade_preserves_content_addresses_ids_and_orphans() -> Result<()> {
    let (connection, base, changed) = fixture()?;
    super::super::run(&connection, &Cancellation::default())?;
    assert_eq!(
        connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
        5
    );
    for (id, expected) in [(11, base), (22, changed)] {
        assert_eq!(
            blobs::read(&connection, id, expected.len(), &Cancellation::default())?,
            expected
        );
    }
    assert_eq!(
        chunk_read::Reader::new(&connection).load(99)?,
        vec![b'a'; 8192]
    );
    assert_eq!(
        connection.query_row("SELECT checkpoint FROM names WHERE name='saved'", [], |r| r
            .get::<_, i64>(0))?,
        7
    );
    assert_eq!(
        connection.query_row("SELECT base FROM chunk_data WHERE id=22", [], |r| r
            .get::<_, i64>(0))?,
        11
    );
    assert_eq!(
        connection.query_row("SELECT count(*) FROM chunk_data", [], |r| r
            .get::<_, i64>(0))?,
        3
    );
    Ok(())
}

#[test]
fn corrupt_content_and_cancellation_preserve_old_payloads_and_version() -> Result<()> {
    for corrupt in [true, false] {
        let (connection, _, _) = fixture()?;
        let cancel = Cancellation::default();
        if corrupt {
            connection.execute("UPDATE chunk_data SET hash=zeroblob(32) WHERE id=11", [])?;
        } else {
            let signal = cancel.clone();
            connection.update_hook(Some(
                move |_: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
                    if table == "chunk_data" {
                        signal.cancel();
                    }
                },
            ))?;
        }
        let before = stored(&connection)?;
        assert!(super::super::run(&connection, &cancel).is_err());
        connection.update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>)?;
        assert_eq!(stored(&connection)?, before);
        assert_eq!(
            connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
            4
        );
        assert!(connection.is_autocommit());
    }
    Ok(())
}

type StoredChunk = (i64, u8, Option<i64>, Vec<u8>);

fn stored(connection: &Connection) -> Result<Vec<StoredChunk>> {
    Ok(connection
        .prepare("SELECT id,codec,base,content FROM chunk_data ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?)
}
