// Purpose: 일반 명령의 공통 증분 동기화와 초기 생성·실패·우회 계약 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn all_secondary_commands_synchronize_before_running() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn first() {}")?;
    let first = project.build(false)?;
    for (index, command) in [
        Command::Status {
            doctor: false,
            refresh: Refresh::Metadata,
        },
        Command::Status {
            doctor: true,
            refresh: Refresh::Metadata,
        },
        Command::Languages {
            refresh: Refresh::Metadata,
        },
        Command::Cache {
            request: CacheRequest::List,
            refresh: Refresh::Metadata,
        },
    ]
    .into_iter()
    .enumerate()
    {
        project.write("main.rs", &format!("fn version_{index}() {{}}"))?;
        let mut sink = Collected::default();
        project.engine.execute(
            &project.options(),
            command,
            &Cancellation::default(),
            &mut sink,
        )?;
        let metadata = sink.metadata.ok_or("metadata")?;
        assert_eq!(
            metadata.generation,
            Some(first.generation + index as i64 + 1)
        );
        assert_eq!(metadata.freshness, "metadata_checked");
        assert_eq!(metadata.coverage.symbols, 2);
    }
    Ok(())
}

#[test]
fn failed_synchronization_stops_queries_but_diagnostics_and_cleanup_work() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn first() {}")?;
    let first = project.build(false)?;
    project.write("main.rs", "fn broken( {")?;
    let mut sink = Collected::default();
    let error = project
        .engine
        .query(
            &project.options(),
            &Query::Symbols {
                file: "main.rs".into(),
            },
            &QueryOptions::default(),
            &Cancellation::default(),
            &mut sink,
        )
        .expect_err("must synchronize");
    assert_eq!(error.kind, ErrorKind::Operation);
    assert!(sink.rows.is_empty());
    project.engine.status(
        &project.options(),
        false,
        &Cancellation::default(),
        &mut sink,
    )?;
    let state = sink
        .metadata
        .ok_or("diagnostic metadata")?
        .history
        .ok_or("history")?;
    assert!(state.analysis_error.is_some());
    assert_ne!(Some(state.checkpoint), state.analysis_checkpoint);
    let mut cached = Collected::default();
    project.engine.execute(
        &project.options(),
        Command::Status {
            doctor: true,
            refresh: Refresh::None,
        },
        &Cancellation::default(),
        &mut cached,
    )?;
    assert_eq!(
        cached.metadata.as_ref().ok_or("metadata")?.generation,
        Some(first.generation)
    );
    assert_eq!(
        cached.metadata.as_ref().ok_or("metadata")?.freshness,
        "not_checked"
    );
    assert!(
        cached
            .rows
            .iter()
            .any(|row| matches!(row, QueryRow::Status(status) if status.last_update == "failed"))
    );
    project.write("main.rs", "fn first() {}")?;
    let mut recovered = Collected::default();
    project.engine.status(
        &project.options(),
        false,
        &Cancellation::default(),
        &mut recovered,
    )?;
    assert_eq!(
        recovered.metadata.ok_or("recovered metadata")?.generation,
        Some(first.generation + 1)
    );
    assert!(recovered.rows.iter().any(|row| matches!(row, QueryRow::Status(status) if status.last_update == "complete" && status.last_error.is_none())));
    project.write("main.rs", "fn broken( {")?;
    project.engine.cache(
        &project.options(),
        CacheRequest::Clean { all: false },
        &Cancellation::default(),
        &mut Collected::default(),
    )?;
    assert!(!std::path::Path::new(&first.cache).exists());
    Ok(())
}

#[test]
fn global_reports_do_not_create_an_initial_project_index() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn broken( {")?;
    for command in [
        Command::Languages {
            refresh: Refresh::Metadata,
        },
        Command::Cache {
            request: CacheRequest::List,
            refresh: Refresh::Metadata,
        },
    ] {
        let mut sink = Collected::default();
        project.engine.execute(
            &project.options(),
            command,
            &Cancellation::default(),
            &mut sink,
        )?;
        assert_eq!(sink.metadata.ok_or("metadata")?.generation, None);
    }
    let error = project
        .engine
        .status(
            &project.options(),
            false,
            &Cancellation::default(),
            &mut Collected::default(),
        )
        .expect_err("build is required");
    assert_eq!(error.kind, ErrorKind::NotFound);
    Ok(())
}
