// Purpose: 초기·증분·전체 결과 일관성과 공개 실패 rollback 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;
use serde_json::Value;

#[test]
fn failure_during_staged_publication_preserves_the_last_complete_generation() -> TestResult {
    for table in ["lookup_rows", "relation_blocks"] {
        let project = Project::new()?;
        project.write(
            "main.rs",
            "fn target() {} fn caller() { target(); target(); }",
        )?;
        let first = project.build(false)?;
        let database =
            rusqlite::Connection::open(std::path::Path::new(&first.cache).join("index.sqlite"))?;
        postings::register(&database)?;
        let proofs=|c:&rusqlite::Connection|->std::result::Result<Vec<(String,String,bool)>,rusqlite::Error>{
            c.prepare("SELECT kind,lookup_key,found FROM lookup_dependencies ORDER BY fact_id,kind,lookup_key")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect()
        };
        let before = proofs(&database)?;
        database.execute_batch(&format!("CREATE TRIGGER fail_publication BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'fixture publication failure'); END;"))?;
        project.write(
            "main.rs",
            "fn changed() {} fn caller() { changed(); changed(); }",
        )?;
        assert_eq!(
            project
                .build(false)
                .err()
                .ok_or("expected publication failure")?
                .kind,
            ErrorKind::Operation
        );
        let cached = project.query(Query::Deps {
            selector: selector("main.rs", "caller"),
        })?;
        assert_eq!(
            cached.metadata.as_ref().ok_or("metadata")?.generation,
            Some(first.generation)
        );
        assert!(
            relationships(&cached).iter().any(|e| e
                .target
                .as_ref()
                .is_some_and(|t| t.name == "target")
                && e.count == 2)
        );
        assert_eq!(before, proofs(&database)?);
        database.execute_batch("DROP TRIGGER fail_publication;")?;
        assert_eq!(project.build(false)?.parsed_files, 1);
        let incremental = project.query(Query::Deps {
            selector: selector("main.rs", "caller"),
        })?;
        assert!(
            relationships(&incremental).iter().any(|e| e
                .target
                .as_ref()
                .is_some_and(|t| t.name == "changed")
                && e.count == 2)
        );
        project.build(true)?;
        let full = project.query(Query::Deps {
            selector: selector("main.rs", "caller"),
        })?;
        assert_eq!(
            relationships(&incremental)[0]
                .target
                .as_ref()
                .ok_or("target")?
                .logical_key,
            relationships(&full)[0]
                .target
                .as_ref()
                .ok_or("target")?
                .logical_key
        );
    }
    Ok(())
}

#[test]
fn explicit_root_queries_never_create_an_initial_index() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn target() {}")?;
    let mut options = project.options();
    options.root = Some(project.root.clone());
    let error = project
        .engine
        .query(
            &options,
            &Query::Symbols {
                file: "main.rs".into(),
            },
            &QueryOptions::default(),
            &Cancellation::default(),
            &mut Collected::default(),
        )
        .expect_err("initial indexing requires build");
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert!(!project.directory.join("cache").exists());
    Ok(())
}

#[test]
fn initialized_history_retries_analysis_after_a_failed_first_generation() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn broken( {")?;
    assert!(project.build(false).is_err());
    project.write("main.rs", "fn fixed() {}")?;
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
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="fixed"))
    );
    assert_eq!(
        sink.metadata
            .ok_or("metadata")?
            .history
            .ok_or("history")?
            .checkpoint,
        "c2"
    );
    Ok(())
}

#[test]
fn unchanged_oversized_inputs_do_not_create_new_generations() -> TestResult {
    let project = Project::new()?;
    project.write(
        "large.rs",
        &format!("// {}\nfn target() {{}}", "a".repeat(128)),
    )?;
    let mut options = project.options();
    options.overrides.max_file_bytes = Some(32);
    let first = project.engine.init(
        &options,
        InitOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    let next = project.engine.init(
        &options,
        InitOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    assert_eq!(first.coverage.skipped_files, 1);
    assert_eq!(first.generation, next.generation);
    assert!(!next.changed);
    Ok(())
}

#[test]
fn content_reuse_preserves_declared_module_identity() -> TestResult {
    let project = Project::new()?;
    let source = "package sample.api; class Target { void target() {} }";
    project.write("nested/Target.java", source)?;
    project.build(false)?;
    let before = project.query(Query::Symbols {
        file: "nested/Target.java".into(),
    })?;
    let before = serde_json::to_value(&before.rows[0])?;
    assert_eq!(before["module"], "sample.api");
    project.write("nested/Target.java", source)?;
    let reused = project.build(false)?;
    assert_eq!(reused.parsed_files, 0);
    let after = project.query(Query::Symbols {
        file: "nested/Target.java".into(),
    })?;
    assert_eq!(
        serde_json::to_value(&after.rows[0])?["module"],
        "sample.api"
    );
    Ok(())
}

#[test]
fn explain_preserves_unresolved_expressions_and_respects_relation_filters() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn caller(x: MissingType) { absent(); }")?;
    project.build(false)?;
    let mut sink = Collected::default();
    project.engine.query(
        &project.options(),
        &Query::Explain {
            selector: selector("main.rs", "caller"),
        },
        &QueryOptions {
            refresh: Refresh::None,
            relations: vec![RelationKind::Calls],
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut sink,
    )?;
    let edges = relationships(&sink);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].resolution, "unresolved");
    let edge = serde_json::to_value(edges[0])?;
    assert_eq!(edge["expression"], "absent");
    assert_eq!(edge["provider_version"], VERSION);
    Ok(())
}

fn normalize(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("id");
            object.remove("owner");
            for value in object.values_mut() {
                normalize(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize(value);
            }
        }
        _ => {}
    }
}

#[test]
fn adding_and_deleting_targets_matches_a_full_rebuild() -> TestResult {
    let project = Project::new()?;
    project.write(
        "caller.ts",
        "import { target } from './api'; function caller() { target(); }",
    )?;
    project.build(false)?;
    for source in [Some("export function target() {}"), None] {
        if let Some(source) = source {
            project.write("api.ts", source)?;
        } else {
            std::fs::remove_file(project.root.join("api.ts"))?;
        }
        project.build(false)?;
        let query = || Query::Explain {
            selector: selector("caller.ts", "caller"),
        };
        let mut incremental = serde_json::to_value(project.query(query())?.rows)?;
        project.build(true)?;
        let mut full = serde_json::to_value(project.query(query())?.rows)?;
        normalize(&mut incremental);
        normalize(&mut full);
        assert_eq!(incremental, full);
    }
    Ok(())
}

#[test]
fn stored_relation_registry_is_reported_until_an_explicit_refresh_rebuilds_it() -> TestResult {
    let p = Project::new()?;
    p.write("lib.rs", "pub fn target() {}")?;
    let report = p.build(false)?;
    let connection =
        rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    connection.execute("UPDATE metadata SET relation_registry_version=1", [])?;
    drop(connection);
    let query = || Query::Symbols {
        file: "lib.rs".into(),
    };
    assert_eq!(
        p.query(query())?
            .metadata
            .ok_or("metadata")?
            .relation_registry_version,
        1
    );
    let refreshed = p.build(false)?;
    assert!(refreshed.changed);
    assert_eq!(refreshed.parsed_files, 1);
    assert_eq!(
        p.query(query())?
            .metadata
            .ok_or("metadata")?
            .relation_registry_version,
        RELATION_REGISTRY_VERSION
    );
    Ok(())
}
