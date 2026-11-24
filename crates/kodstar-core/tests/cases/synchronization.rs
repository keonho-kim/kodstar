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
fn partial_synchronization_keeps_diagnostics_recovery_and_cleanup_usable() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn first() {}")?;
    let first = project.build(false)?;
    project.write("main.rs", "fn broken( {")?;
    let mut sink = Collected::default();
    let summary = project.engine.query(
        &project.options(),
        &Query::Symbols {
            file: "main.rs".into(),
        },
        &QueryOptions::default(),
        &Cancellation::default(),
        &mut sink,
    )?;
    assert!(!summary.complete);
    assert!(sink.rows.is_empty());
    let mut diagnostic = Collected::default();
    project.engine.execute(
        &project.options(),
        Command::Status {
            doctor: true,
            refresh: Refresh::None,
        },
        &Cancellation::default(),
        &mut diagnostic,
    )?;
    assert_eq!(
        diagnostic.metadata.as_ref().ok_or("metadata")?.generation,
        Some(first.generation + 1)
    );
    assert!(
        diagnostic
            .rows
            .iter()
            .any(|r| matches!(r, QueryRow::Diagnostic(d) if d.code == "source_syntax_partial"))
    );
    let state = diagnostic
        .metadata
        .ok_or("metadata")?
        .history
        .ok_or("history")?;
    assert_eq!(Some(state.checkpoint), state.analysis_checkpoint);
    project.write("main.rs", "fn first() {}")?;
    let mut recovered = Collected::default();
    project.engine.status(
        &project.options(),
        false,
        &Cancellation::default(),
        &mut recovered,
    )?;
    assert_eq!(
        recovered.metadata.ok_or("metadata")?.generation,
        Some(first.generation + 2)
    );
    assert!(recovered.summary.ok_or("summary")?.complete);
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
