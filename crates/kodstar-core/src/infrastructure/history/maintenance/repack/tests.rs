// Purpose: 기존 이력 재압축의 절약·깊이·원문·rollback 검증.
// Pattern: Storage Regression Test.
// Usage: 명시적 유지보수의 실제 SQLite transaction을 검증.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/repack.rs.

use super::*;
use crate::infrastructure::history::content::{blobs, chunks};
use std::io::Cursor;

fn fixture() -> Result<(Connection, Vec<Vec<u8>>)> {
    let c = Connection::open_in_memory()?;
    c.execute_batch(include_str!("../../schema.sql"))?;
    c.execute_batch(include_str!("../../metadata.sql"))?;
    c.execute_batch(chunks::SCHEMA)?;
    chunks::register(&c)?;
    c.execute_batch("PRAGMA foreign_keys=ON;")?;
    let mut state = 42u64;
    let original: Vec<u8> = (0..8192)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let mut versions = Vec::new();
    for n in 0..20 {
        let mut raw = original.clone();
        raw[100..104].copy_from_slice(&(n as u32).to_le_bytes());
        blobs::Writer::new(&c)?.put(
            Cursor::new(&raw),
            raw.len() as u64,
            None,
            &Cancellation::default(),
        )?;
        versions.push(raw);
    }
    Ok((c, versions))
}

#[test]
fn old_independent_versions_share_one_base_without_losing_any_blob() -> Result<()> {
    let (c, versions) = fixture()?;
    let before: usize = c.query_row("SELECT sum(length(content)) FROM chunk_data", [], |r| {
        r.get(0)
    })?;
    run(&c, &Cancellation::default())?;
    let after: usize = c.query_row("SELECT sum(length(content)) FROM chunk_data", [], |r| {
        r.get(0)
    })?;
    assert!(after < before / 5, "{before} -> {after}");
    for (n, expected) in versions.iter().enumerate() {
        assert_eq!(
            blobs::read(&c, n as i64 + 1, expected.len(), &Cancellation::default())?,
            *expected
        );
    }
    let bad:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM chunk_data d JOIN chunk_data b ON b.id=d.base WHERE b.codec=2 OR b.id>=d.id)",[],|r|r.get(0))?;
    assert!(!bad);
    Ok(())
}

#[test]
fn failed_repack_restores_all_previous_physical_payloads() -> Result<()> {
    let (c, _) = fixture()?;
    let before: Vec<Vec<u8>> = c
        .prepare("SELECT content FROM chunk_data ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    c.execute_batch("CREATE TRIGGER fail_repack BEFORE UPDATE ON chunk_data WHEN NEW.id>3 BEGIN SELECT RAISE(ABORT,'repack fixture failure'); END;")?;
    assert!(run(&c, &Cancellation::default()).is_err());
    let after: Vec<Vec<u8>> = c
        .prepare("SELECT content FROM chunk_data ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    assert_eq!(before, after);
    assert!(c.is_autocommit());
    Ok(())
}

#[test]
fn cancellation_after_a_write_rolls_back_reencoding() -> Result<()> {
    let (c, versions) = fixture()?;
    let cancel = Cancellation::default();
    let signal = cancel.clone();
    c.create_scalar_function(
        "cancel_repack",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        move |_| {
            signal.cancel();
            Ok(0)
        },
    )?;
    c.execute_batch("CREATE TRIGGER cancel_repack AFTER UPDATE ON chunk_data WHEN NEW.id=3 BEGIN SELECT cancel_repack(); END;")?;
    assert!(run(&c, &cancel).is_err());
    assert_eq!(
        c.query_row("SELECT count(*) FROM chunk_data WHERE codec=2", [], |r| {
            r.get::<_, usize>(0)
        })?,
        0
    );
    for (n, expected) in versions.iter().enumerate() {
        assert_eq!(
            blobs::read(&c, n as i64 + 1, expected.len(), &Cancellation::default())?,
            *expected
        );
    }
    Ok(())
}
