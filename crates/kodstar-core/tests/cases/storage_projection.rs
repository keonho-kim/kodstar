// Purpose: 중복 저장 제거 뒤 원래 구문·대표 객체·출력 경로와 재해석 보존 검증.
// Pattern: Storage Regression Test.
// Usage: engine 통합 테스트가 논리 결과와 물리 저장 계약을 함께 확인.
// Related: crates/kodstar-core/src/infrastructure/index/schema.sql, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::Query;

#[test]
fn sparse_identity_storage_preserves_merged_definitions_and_reanalysis() -> TestResult {
    let p = Project::new()?;
    p.write("main.c", "int target(int input); int target(int input){return input;} int caller(){return target(1);}")?;
    let initial = p.build(false)?;
    let database = std::path::Path::new(&initial.cache).join("index.sqlite");
    let c = rusqlite::Connection::open(&database)?;
    crate::support::postings::register(&c)?;
    let aliases: u64 = c.query_row("SELECT COUNT(*) FROM symbol_aliases", [], |r| r.get(0))?;
    assert_eq!(
        aliases, 1,
        "only the additional declaration needs a stored mapping"
    );
    let logical: u64 = c.query_row("SELECT COUNT(*) FROM symbol_identities", [], |r| r.get(0))?;
    let symbols: u64 = c.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
    assert_eq!(logical, symbols);
    let duplicate: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM symbols WHERE resolved_key=syntax_key OR resolved_qualified_name=syntax_qualified_name OR resolved_kind=syntax_kind)", [], |r| r.get(0))?;
    assert!(
        !duplicate,
        "equal values must use the original stored field"
    );
    let rows = p.query(Query::Deps {
        selector: selector("main.c", "caller"),
    })?;
    let target = relationships(&rows)
        .into_iter()
        .find_map(|r| r.target.as_ref())
        .ok_or("target")?;
    assert_eq!(target.definitions.len(), 2);
    drop(c);
    p.write(
        "main.c",
        "int target(int input); int caller(){return target(1);}",
    )?;
    let partial = p.build(false)?;
    let snapshot = normalized_index(&partial)?;
    assert_eq!(snapshot, normalized_index(&p.build(true)?)?);
    let c = rusqlite::Connection::open(database)?;
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM symbol_aliases", [], |r| r
            .get::<_, u64>(0))?,
        0
    );
    Ok(())
}

#[test]
fn derived_path_display_preserves_non_utf8_byte_identity() -> TestResult {
    let p = Project::new()?;
    p.write("api.py", "def target(): pass\n")?;
    let report = p.build(false)?;
    let c = rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    crate::support::postings::register(&c)?;
    // Exercise stored Unix bytes independently of filesystems that reject invalid UTF-8.
    c.execute(
        "UPDATE files SET path=?1 WHERE language='python'",
        [b"api-\xff.py".as_slice()],
    )?;
    let (bytes, display): (Vec<u8>, String) = c.query_row(
        "SELECT path,display_path FROM files WHERE language='python'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(bytes, b"api-\xff.py");
    assert_eq!(display, "api-\u{fffd}.py");
    let stored: bool = c.query_row("SELECT NOT EXISTS(SELECT 1 FROM pragma_table_xinfo('files_rows') WHERE name='display_path')", [], |r| r.get(0))?;
    assert!(
        stored,
        "display text should be computed without storing a second path"
    );
    let generation: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_xinfo('files') WHERE name='seen_generation')",
        [],
        |r| r.get(0),
    )?;
    assert!(!generation, "file generations have no reader");
    Ok(())
}
