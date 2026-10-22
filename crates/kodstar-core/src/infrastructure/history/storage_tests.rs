// Purpose: 장기 작은 수정·큰 삽입의 공간 증가량과 바이트·구간 복원 검증.
// Pattern: Storage Regression.
// Usage: history adapter의 전체 쓰기·복원·검증 경로를 결정적 fixture로 실행.
// Related: crates/kodstar-core/src/infrastructure/history/content/blobs.rs, crates/kodstar-core/src/infrastructure/history/content/reader.rs.

use super::content::{blobs, chunks, reader};
use super::*;
use std::io::Cursor;

fn database() -> Result<Connection> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(include_str!("schema.sql"))?;
    connection.execute_batch(chunks::SCHEMA)?;
    chunks::register(&connection)?;
    connection.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(connection)
}

fn noise(length: usize) -> Vec<u8> {
    let mut value = 42u64;
    (0..length)
        .map(|_| {
            value ^= value << 13;
            value ^= value >> 7;
            value ^= value << 17;
            value as u8
        })
        .collect()
}

#[test]
fn thousand_small_edits_preserve_every_version_with_one_level_deltas() -> Result<()> {
    let connection = database()?;
    let cancel = Cancellation::default();
    let original = noise(8192);
    let first = blobs::put(&connection, Cursor::new(&original), None, &cancel)?;
    let mut previous = Some(first);
    let mut versions = Vec::new();
    for index in 0..1000usize {
        let mut changed = original.clone();
        changed[index..index + 4].copy_from_slice(&(index as u32).to_le_bytes());
        let id = blobs::put(&connection, Cursor::new(&changed), previous, &cancel)?;
        assert_eq!(blobs::read(&connection, id, 8192, &cancel)?, changed);
        versions.push(id);
        previous = Some(id);
    }
    for (index, id) in versions.into_iter().enumerate() {
        let mut expected = original.clone();
        expected[index..index + 4].copy_from_slice(&(index as u32).to_le_bytes());
        assert_eq!(blobs::read(&connection, id, 8192, &cancel)?, expected);
    }
    assert_eq!(blobs::read(&connection, first, 8192, &cancel)?, original);
    let stored: u64 =
        connection.query_row("SELECT sum(length(content)) FROM chunk_data", [], |r| {
            r.get(0)
        })?;
    assert!(stored < 100_000, "unexpected physical payload: {stored}");
    let nested:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM chunk_data d JOIN chunk_data b ON b.id=d.base WHERE b.base IS NOT NULL)",[],|r|r.get(0))?;
    assert!(!nested);
    Ok(())
}

#[test]
fn large_insertions_share_content_and_random_reads_cross_variable_boundaries() -> Result<()> {
    let connection = database()?;
    let cancel = Cancellation::default();
    let original = noise(1024 * 1024);
    let first = blobs::put(&connection, Cursor::new(&original), None, &cancel)?;
    let before: u64 =
        connection.query_row("SELECT sum(length(content)) FROM chunk_data", [], |r| {
            r.get(0)
        })?;
    let changed = [b"inserted prefix\n".as_slice(), &original].concat();
    let second = blobs::put(&connection, Cursor::new(&changed), Some(first), &cancel)?;
    let after: u64 =
        connection.query_row("SELECT sum(length(content)) FROM chunk_data", [], |r| {
            r.get(0)
        })?;
    assert!(
        after - before < 100_000,
        "insertion rewrote shared content: {}",
        after - before
    );
    for (id, expected) in [(first, &original), (second, &changed)] {
        assert_eq!(
            blobs::read(&connection, id, expected.len(), &cancel)?,
            *expected
        );
        let mut reader = reader::Reader::verified(&connection, id)?;
        let mut buffer = [0; 10000];
        for offset in [0, 2043, 8190, 32765, 65534, 900000, expected.len() - 17] {
            let size = reader.read_at(offset as u64, &mut buffer, &cancel)?;
            assert_eq!(&buffer[..size], &expected[offset..offset + size]);
        }
    }
    Ok(())
}

#[test]
fn invalid_delta_base_is_rejected_without_following_a_chain() -> Result<()> {
    let connection = database()?;
    let cancel = Cancellation::default();
    let original = noise(8192);
    let first = blobs::put(&connection, Cursor::new(&original), None, &cancel)?;
    let mut changed = original.clone();
    changed[42] ^= 1;
    let second = blobs::put(&connection, Cursor::new(&changed), Some(first), &cancel)?;
    connection.execute_batch(
        "PRAGMA ignore_check_constraints=ON;
        UPDATE chunk_data SET base=id WHERE codec=2;",
    )?;
    assert!(blobs::read(&connection, second, 8192, &cancel).is_err());
    Ok(())
}
