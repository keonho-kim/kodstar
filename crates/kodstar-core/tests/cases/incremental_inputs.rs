// Purpose: 이력 전용 입력 변경이 분석 해석과 정상 세대를 불필요하게 다시 만들지 않는지 검증.
// Pattern: Integration Test.
// Usage: engine 테스트가 언어별 소형 프로젝트의 변경과 full 결과를 비교.
// Related: crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;

#[test]
fn history_only_edits_and_deletions_reuse_relationships_across_languages() -> TestResult {
    for (path, source) in [
        ("main.py", "def target(): pass\ndef caller(): target()\n"),
        (
            "main.js",
            "function target() {} function caller() { target(); }",
        ),
        ("main.rs", "fn target() {} fn caller() { target(); }"),
        (
            "main.go",
            "package main\nfunc target() {}\nfunc caller() { target() }\n",
        ),
    ] {
        let p = Project::new()?;
        p.write(path, source)?;
        p.write("README.md", "first")?;
        p.build(false)?;
        for removed in [false, true] {
            if removed {
                std::fs::remove_file(p.root.join("README.md"))?;
            } else {
                p.write("README.md", "second")?;
            }
            let mut phases = Vec::new();
            let updated = p.engine.init(
                &p.options(),
                InitOptions::default(),
                &Cancellation::default(),
                &mut |progress| {
                    phases.push(progress.phase);
                    Ok(())
                },
            )?;
            assert!(!phases.contains(&"resolve"), "{path}: {phases:?}");
            assert_eq!(updated.parsed_files, 0);
            assert!(updated.history.as_ref().ok_or("history")?.created);
            assert_eq!(
                normalized_index(&updated)?,
                normalized_index(&p.build(true)?)?
            );
        }
    }
    Ok(())
}

#[test]
fn replacing_or_deleting_analyzed_input_still_invalidates_its_consumers() -> TestResult {
    let p = Project::new()?;
    p.write("api.py", "def target(): pass\n")?;
    p.write(
        "main.py",
        "from api import target\ndef caller(): target()\n",
    )?;
    p.build(false)?;
    std::fs::rename(p.root.join("api.py"), p.root.join("api.md"))?;
    let changed = p.build(false)?;
    assert_eq!(
        normalized_index(&changed)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn snapshot_normalization_preserves_lookup_meaning_across_file_id_allocation() -> TestResult {
    let p = Project::new()?;
    p.write("main.py", "def target(): pass\ndef caller(): target()\n")?;
    let first = p.build(false)?;
    let before = normalized_index(&first)?;
    let database =
        rusqlite::Connection::open(std::path::Path::new(&first.cache).join("index.sqlite"))?;
    postings::register(&database)?;
    database.execute_batch(
        "BEGIN; PRAGMA defer_foreign_keys=ON;
        UPDATE files SET id=id+1000;
        UPDATE symbols SET file_id=file_id+1000;
        UPDATE scopes SET file_id=file_id+1000;
        UPDATE bindings SET file_id=file_id+1000;
        UPDATE facts SET file_id=file_id+1000;
        UPDATE lookup_pages SET file_id=file_id+1000;
        UPDATE relation_blocks SET file_id=file_id+1000;
        UPDATE diagnostics SET file_id=file_id+1000;
        COMMIT;",
    )?;
    packed_mutations::remap_scopes(&database)?;
    assert_eq!(before, normalized_index(&first)?);
    postings::lookup_pages::change_first(&database, None)?;
    assert_ne!(before, normalized_index(&first)?);
    Ok(())
}

#[test]
fn doctor_checks_fact_ownership_inside_compressed_lookup_blocks() -> TestResult {
    let p = Project::new()?;
    p.write("main.py", "def target(): pass\ndef caller(): target()\n")?;
    let initial = p.build(false)?;
    let database =
        rusqlite::Connection::open(std::path::Path::new(&initial.cache).join("index.sqlite"))?;
    postings::register(&database)?;
    // A valid integer block referring to an absent fact bypasses ordinary SQLite foreign keys.
    let payload = [254u8, 255, 255, 255, 255, 255, 255, 255, 255, 1];
    postings::lookup_pages::change_first(&database, Some(&payload))?;
    let error = p
        .engine
        .status(
            &p.options(),
            true,
            &Cancellation::default(),
            &mut Collected::default(),
        )
        .expect_err("doctor accepted a dangling packed dependency");
    assert!(error.to_string().contains("lookup dependency"), "{error}");
    Ok(())
}
