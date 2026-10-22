// Purpose: 초기화·자동 소스 보존과 분석 세대의 연결 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트가 임시 프로젝트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;
use std::fs;

#[test]
fn historical_package_identity_uses_preserved_manifests() -> TestResult {
    let project = Project::new()?;
    project.write(
        "member/Cargo.toml",
        "[package]\nname='member'\nversion='0.1.0'\n",
    )?;
    project.write("member/src/lib.rs", "pub fn preserved() {}")?;
    project.build(false)?;
    let query = Query::Symbols {
        file: "member/src/lib.rs".into(),
    };
    let original = project.query(query.clone())?;
    fs::remove_file(project.root.join("member/Cargo.toml"))?;
    let mut historical = Collected::default();
    project.engine.query(
        &project.options(),
        &query,
        &QueryOptions {
            revision: Some("c1".into()),
            limit: Limit::Unlimited,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut historical,
    )?;
    // Row IDs belong to each analysis generation; package identity and source evidence do not.
    let logical =
        |rows: &[QueryRow]| -> std::result::Result<serde_json::Value, Box<dyn std::error::Error>> {
            let mut value = serde_json::to_value(rows)?;
            for symbol in value.as_array_mut().ok_or("symbol array")? {
                symbol.as_object_mut().ok_or("symbol")?.remove("id");
                symbol.as_object_mut().ok_or("symbol")?.remove("owner");
            }
            Ok(value)
        };
    assert_eq!(logical(&historical.rows)?, logical(&original.rows)?);
    assert!(!project.root.join("member/Cargo.toml").exists());
    Ok(())
}

#[test]
fn initialization_tracks_large_binary_content_and_deduplicates_repeated_queries() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn first() {}")?;
    fs::write(project.root.join("asset.bin"), vec![42u8; 3 * 1024 * 1024])?;
    let report = project.build(false)?;
    let state = report.history.ok_or("missing history state")?;
    assert_eq!(state.checkpoint, "c1");
    assert_eq!(state.tracked_files, 2);
    let db = rusqlite::Connection::open(project.root.join(".kodstar/repository.sqlite"))?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM checkpoints", [], |r| r
            .get::<_, u64>(0))?,
        1
    );
    assert_eq!(
        db.query_row("SELECT max(size) FROM chunk_data", [], |r| r
            .get::<_, usize>(0))?,
        32768
    );
    project.engine.init(
        &project.options(),
        InitOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM checkpoints", [], |r| r
            .get::<_, u64>(0))?,
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM blobs", [], |r| r.get::<_, u64>(0))?,
        2
    );
    Ok(())
}

#[test]
fn analysis_uses_captured_bytes_when_working_source_changes_during_progress() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn captured() {}")?;
    let mut edited = false;
    let report = project.engine.init(
        &project.options(),
        InitOptions::default(),
        &Cancellation::default(),
        &mut |_| {
            if !edited {
                fs::write(project.root.join("main.rs"), "fn edited_later() {}")
                    .map_err(|e| Error::caused_by("edit fixture", e))?;
                edited = true;
            }
            Ok(())
        },
    )?;
    assert!(edited);
    let result = project.query(Query::Symbols {
        file: "main.rs".into(),
    })?;
    assert!(
        result
            .rows
            .iter()
            .any(|row| matches!(row,QueryRow::Symbol(s) if s.name=="captured"))
    );
    let connection =
        rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    assert_eq!(
        connection.query_row("SELECT checkpoint FROM metadata", [], |r| r
            .get::<_, String>(0))?,
        "c1"
    );
    Ok(())
}

#[test]
fn cache_cleanup_preserves_history_and_analysis_can_be_recreated() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn preserved() {}")?;
    let report = project.build(false)?;
    let mut sink = Collected::default();
    project.engine.cache(
        &project.options(),
        CacheRequest::Clean { all: false },
        &Cancellation::default(),
        &mut sink,
    )?;
    assert!(!std::path::Path::new(&report.cache).exists());
    assert!(project.root.join(".kodstar/repository.sqlite").exists());
    let mut sink = Collected::default();
    project.engine.query(
        &project.options(),
        &Query::Symbols {
            file: "main.rs".into(),
        },
        &QueryOptions::default(),
        &Cancellation::default(),
        &mut sink,
    )?;
    assert!(
        sink.rows
            .iter()
            .any(|row| matches!(row,QueryRow::Symbol(s) if s.name=="preserved"))
    );
    Ok(())
}

#[test]
fn unchanged_queries_do_not_rewrite_source_or_analysis_databases() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn stable() {}")?;
    let report = project.build(false)?;
    let path = project.root.join(".kodstar/repository.sqlite");
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
    let index = std::path::Path::new(&report.cache).join("index.sqlite");
    fs::OpenOptions::new()
        .write(true)
        .open(&index)?
        .set_modified(old)?;
    fs::OpenOptions::new()
        .write(true)
        .open(&path)?
        .set_modified(old)?;
    project.engine.query(
        &project.options(),
        &Query::Symbols {
            file: "main.rs".into(),
        },
        &QueryOptions::default(),
        &Cancellation::default(),
        &mut Collected::default(),
    )?;
    assert_eq!(fs::metadata(path)?.modified()?, old);
    assert_eq!(fs::metadata(index)?.modified()?, old);
    Ok(())
}
