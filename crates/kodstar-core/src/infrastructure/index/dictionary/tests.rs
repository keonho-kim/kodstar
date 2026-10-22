// Purpose: 접두어 사전의 Unicode·안정 ID·잘못된 입력 회귀.
// Pattern: Storage Test.
// Usage: dictionary codec과 제한 블록 공개를 검증.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/mod.rs.

use super::*;

#[test]
fn unicode_and_exact_prefixes_keep_every_key_and_id() -> Result<()> {
    let rows = vec![
        (1, "go:모듈".into()),
        (300, "go:모듈:함수".into()),
        (17, "rust:α".into()),
    ];
    let bytes = codec::encode(&rows);
    assert_eq!(codec::decode(&bytes, rows.len())?, rows);
    assert!(codec::decode(&bytes[..bytes.len() - 1], rows.len()).is_err());
    let mut extra = bytes;
    extra.push(0);
    assert!(codec::decode(&extra, rows.len()).is_err());
    Ok(())
}

#[test]
fn insertion_splits_without_changing_existing_ids() -> Result<()> {
    let c = Connection::open_in_memory()?;
    c.pragma_update(None, "foreign_keys", true)?;
    super::super::packed::register(&c)?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch("CREATE TEMP TABLE resolution_dependencies(kind TEXT,lookup_key TEXT);")?;
    for n in 0..100 {
        c.execute(
            "INSERT INTO resolution_dependencies VALUES('module',?1)",
            [format!("pkg:{n:04}")],
        )?;
    }
    publish(&c)?;
    c.execute_batch("DROP TABLE dictionary_pending;")?;
    let old: i64 = c.query_row(
        "SELECT id FROM lookup_keys WHERE kind='module' AND lookup_key='pkg:0050'",
        [],
        |r| r.get(0),
    )?;
    c.execute_batch("DELETE FROM resolution_dependencies; INSERT INTO resolution_dependencies VALUES('module','pkg:0050:a'),('file','same');")?;
    publish(&c)?;
    c.execute_batch("DROP TABLE dictionary_pending;")?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM lookup_keys", [], |r| r
            .get::<_, usize>(0))?,
        102
    );
    assert_eq!(
        c.query_row(
            "SELECT lookup_key FROM lookup_keys WHERE id=?1",
            [old],
            |r| r.get::<_, String>(0)
        )?,
        "pkg:0050"
    );
    assert_eq!(c.query_row("SELECT count(*) FROM lookup_keys WHERE kind='module' AND substr(lookup_key,1,7)='pkg:005'",[],|r|r.get::<_,usize>(0))?,11);
    Ok(())
}

#[test]
fn shared_prefixes_cannot_expand_a_block_past_its_byte_budget() -> Result<()> {
    let c = Connection::open_in_memory()?;
    super::super::packed::register(&c)?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch("CREATE TEMP TABLE resolution_dependencies(kind TEXT,lookup_key TEXT);")?;
    for n in 0..40 {
        c.execute(
            "INSERT INTO resolution_dependencies VALUES('expression',?1)",
            [format!("{}:{n:03}", "모듈".repeat(2000))],
        )?;
    }
    publish(&c)?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM lookup_keys", [], |r| r
            .get::<_, usize>(0))?,
        40
    );
    assert!(
        c.query_row("SELECT count(*) FROM dictionary_blocks", [], |r| r
            .get::<_, usize>(0))?
            > 1
    );
    let malicious = vec![
        (1, "x".repeat(40000)),
        (2, format!("{}y", "x".repeat(40000))),
    ];
    assert!(codec::decode(&codec::encode(&malicious), 2).is_err());
    Ok(())
}

#[test]
fn a_large_key_does_not_duplicate_sparse_bounds_or_exceed_the_original_row_limit() -> Result<()> {
    let c = Connection::open_in_memory()?;
    c.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 256 * 1024)?;
    super::super::packed::register(&c)?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch("CREATE TEMP TABLE resolution_dependencies(kind TEXT,lookup_key TEXT);")?;
    let key = "가".repeat(40000);
    c.execute(
        "INSERT INTO resolution_dependencies VALUES('expression',?1)",
        [&key],
    )?;
    publish(&c)?;
    c.execute_batch("DROP TABLE dictionary_pending;")?;
    let id: i64 = c.query_row(
        "SELECT id FROM lookup_keys WHERE kind='expression' AND lookup_key=?1",
        [&key],
        |r| r.get(0),
    )?;
    assert_eq!(
        c.query_row(
            "SELECT lookup_key FROM lookup_keys WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0)
        )?,
        key
    );
    super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    publish(&c)?;
    c.execute_batch("DROP TABLE dictionary_pending;")?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM dictionary_ids", [], |r| r
            .get::<_, usize>(0))?,
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM lookup_keys WHERE kind='EXPRESSION' COLLATE NOCASE",
            [],
            |r| r.get::<_, usize>(0)
        )?,
        1
    );
    Ok(())
}
