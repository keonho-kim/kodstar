// Purpose: 동일 체크포인트 분석 재사용의 무효화·검증·보고 계약 확인.
// Pattern: Integration Test.
// Usage: engine 테스트가 독립 저장소에서 일반·verify·full 갱신을 비교.
// Related: crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;
use std::{fs, path::Path};

fn refresh(project: &Project, options: InitOptions) -> Result<InitReport> {
    project.engine.init(
        &project.options(),
        options,
        &Cancellation::default(),
        &mut |_| Ok(()),
    )
}

#[test]
fn metadata_reuse_preserves_coverage_and_still_captures_source_changes() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn first() {} fn caller() { first(); }")?;
    p.write("asset.bin", "unsupported")?;
    p.write("large.rs", &" ".repeat(2 * 1024 * 1024 + 1))?;
    let initial = p.build(false)?;
    let expected = normalized_index(&initial)?;
    let reused = refresh(&p, InitOptions::default())?;
    assert!(!reused.changed);
    assert_eq!(reused.generation, initial.generation);
    assert_eq!(reused.indexed_at, initial.indexed_at);
    assert_eq!(reused.parsed_files, 0);
    assert_eq!(reused.deleted_files, 0);
    assert_eq!(reused.coverage.reused_files, initial.coverage.scanned_files);
    assert_eq!(reused.coverage.skipped_files, 1);
    assert_eq!(reused.coverage.unsupported_files, 1);
    let verified = refresh(
        &p,
        InitOptions {
            verify: true,
            ..InitOptions::default()
        },
    )?;
    assert!(!verified.changed);
    assert_eq!(verified.generation, initial.generation);
    assert_eq!(verified.parsed_files, 0);
    assert_eq!(
        serde_json::to_value(&verified.coverage)?,
        serde_json::to_value(&reused.coverage)?
    );
    assert_eq!(expected, normalized_index(&verified)?);

    p.write("main.rs", "fn second() {} fn caller() { second(); }")?;
    fs::remove_file(p.root.join("asset.bin"))?;
    let changed = refresh(&p, InitOptions::default())?;
    assert!(changed.changed);
    assert_eq!(changed.generation, initial.generation + 1);
    assert_eq!(changed.parsed_files, 1);
    assert_eq!(changed.deleted_files, 1);
    assert_eq!(changed.history.as_ref().ok_or("history")?.checkpoint, "c2");
    assert_eq!(
        normalized_index(&changed)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn incomplete_metadata_and_environment_changes_cannot_reuse_a_generation() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn stable() {}")?;
    let initial = p.build(false)?;
    let connection = rusqlite::Connection::open(Path::new(&initial.cache).join("index.sqlite"))?;
    for state in ["failed", "cancelled"] {
        connection.execute(
            "UPDATE metadata SET last_update=?1,last_error='interrupted'",
            [state],
        )?;
        refresh(&p, InitOptions::default())?;
        let (status, error): (String, Option<String>) =
            connection.query_row("SELECT last_update,last_error FROM metadata", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        assert_eq!(status, "complete");
        assert_eq!(error, None);
    }
    let mut generation = initial.generation;
    for assignment in [
        "extraction_version=0",
        "relation_registry_version=0",
        "repository='other'",
        "config_hash='other'",
    ] {
        connection.execute(&format!("UPDATE metadata SET {assignment}"), [])?;
        let report = refresh(&p, InitOptions::default())?;
        generation += 1;
        assert_eq!(report.generation, generation);
        assert_eq!(report.parsed_files, 1);
        assert!(report.changed);
    }
    let full = refresh(
        &p,
        InitOptions {
            full: true,
            ..InitOptions::default()
        },
    )?;
    assert_eq!(full.parsed_files, 1);
    assert_eq!(full.generation, generation + 1);
    Ok(())
}

#[test]
fn explicit_verification_does_not_skip_stored_content_integrity_checks() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn preserved() {}")?;
    let initial = p.build(false)?;
    let before = normalized_index(&initial)?;
    let source = rusqlite::Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    source.execute(
        "UPDATE chunk_data SET content=zeroblob(length(content))",
        [],
    )?;
    drop(source);
    for options in [
        InitOptions {
            verify: true,
            ..InitOptions::default()
        },
        InitOptions {
            full: true,
            ..InitOptions::default()
        },
    ] {
        let error = refresh(&p, options).expect_err("corrupt snapshot accepted");
        assert!(error.to_string().contains("checksum mismatch"), "{error}");
        assert_eq!(before, normalized_index(&initial)?);
    }
    Ok(())
}
