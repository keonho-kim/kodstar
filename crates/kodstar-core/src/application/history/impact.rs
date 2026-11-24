// Purpose: 변경 전·후 그래프를 사용한 체크포인트 이후 영향 분석.
// Pattern: Use Case.
// Usage: 공통 query handler가 --since 요청을 전달.
// Related: crates/kodstar-core/src/application/query/mod.rs, crates/kodstar-core/src/application/history/impact/rows.rs.

mod rows;

use crate::application::{
    analysis::{self, InitOptions},
    execution::context::ProjectContext,
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    history::Comparison,
    query::{QueryOptions, Refresh},
    results::{QuerySink, QuerySummary},
};
use crate::infrastructure::{cache::CacheStore, history::checkpoints, index};
use rusqlite::Connection;
use std::time::Instant;

pub(crate) fn run(
    store: &CacheStore,
    project: &mut ProjectContext,
    since: &str,
    options: &QueryOptions,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<QuerySummary> {
    let from = checkpoints::resolve(&project.repository.connection, since)?;
    let to = checkpoints::head(&project.repository.connection)?
        .ok_or_else(|| Error::not_found("current checkpoint is missing"))?;
    if options.refresh == Refresh::None {
        index::require_history(
            &project.connection,
            &project.repository.state()?.repository,
            Some(&checkpoints::display(to)),
        )?;
        let actual: Option<String> =
            project
                .connection
                .query_row("SELECT checkpoint FROM metadata", [], |r| r.get(0))?;
        if actual.as_deref() != Some(checkpoints::display(to).as_str()) {
            return Err(Error::not_found(
                "current checkpoint analysis is stale; retry without --no-refresh",
            ));
        }
    }
    if from == to {
        let mut metadata = crate::application::query::metadata::read(
            &project.connection,
            &project.selected,
            project.refresh,
            project.maintenance.clone(),
        )?;
        let (configuration, extraction): (String, u32) = project.connection.query_row(
            "SELECT config_hash,extraction_version FROM metadata",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if let Some(state) = &mut metadata.history {
            state.comparison = Some(Comparison {
                from: Some(checkpoints::display(from)),
                to: checkpoints::display(to),
                before_coverage: Some(metadata.coverage.clone()),
                after_coverage: Some(metadata.coverage.clone()),
                configuration: Some(configuration),
                extraction_version: Some(extraction),
            });
        }
        project.repository.release();
        sink.begin(&metadata)?;
        let summary = QuerySummary {
            complete: true,
            unresolved_count: metadata.unresolved_count,
            ..QuerySummary::default()
        };
        sink.finish(&summary)?;
        return Ok(summary);
    }
    let mut selected = project.selected.clone();
    selected.snapshot = Some(project.repository.snapshot(from));
    if let Some(state) = &mut selected.history {
        state.checkpoint = checkpoints::display(from);
        state.analysis_checkpoint = Some(state.checkpoint.clone());
    }
    let path = project.cache.directory.join("past.sqlite");
    let writer = project.cache.writer(cancel)?;
    let mut before = index::open(
        &path,
        &selected.root,
        options.refresh != Refresh::None,
        selected.settings.db_cache_bytes,
    )?;
    if options.refresh == Refresh::None {
        index::require_committed(&before)?;
        index::require_history(
            &before,
            &project.repository.state()?.repository,
            Some(&checkpoints::display(from)),
        )?;
        let cached: Option<String> =
            before.query_row("SELECT checkpoint FROM metadata", [], |r| r.get(0))?;
        if cached.as_deref() != Some(checkpoints::display(from).as_str()) {
            return Err(Error::not_found(
                "previous checkpoint analysis is not cached; retry without --no-refresh",
            ));
        }
    } else {
        analysis::run(
            analysis::Session {
                store,
                selected: &selected,
                cache: &project.cache,
                database: &path,
                started: Instant::now(),
            },
            &mut before,
            InitOptions::default(),
            cancel,
            &mut |event| sink.progress(event),
        )?;
    }
    changes(
        &project.repository.connection,
        from,
        to,
        &before,
        &project.connection,
        cancel,
    )?;
    for connection in [&before, &project.connection] {
        index::limit_reads(connection, selected.settings.memory_budget)?;
        index::observe_cancellation(connection, cancel)?;
    }
    let before = before.transaction()?;
    let after = project.connection.transaction()?;
    let before_environment: (String, u32, u32) = before.query_row(
        "SELECT config_hash,extraction_version,relation_registry_version FROM metadata",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let after_environment: (String, u32, u32) = after.query_row(
        "SELECT config_hash,extraction_version,relation_registry_version FROM metadata",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if before_environment != after_environment {
        return Err(Error::operation(
            "comparison analysis environments differ; retry without --no-refresh",
        ));
    }
    let mut metadata = crate::application::query::metadata::read(
        &after,
        &project.selected,
        project.refresh,
        project.maintenance.clone(),
    )?;
    if let Some(state) = &mut metadata.history {
        state.comparison = Some(Comparison {
            before_coverage: Some(index::coverage(&before)?),
            after_coverage: Some(index::coverage(&after)?),
            configuration: Some(before_environment.0),
            extraction_version: Some(before_environment.1),
            from: Some(checkpoints::display(from)),
            to: checkpoints::display(to),
        });
    }
    drop(writer);
    project.repository.release();
    sink.begin(&metadata)?;
    let mut output = rows::Output::new(options, &selected.settings, cancel, sink);
    output.emit(&before, &selected, &checkpoints::display(from))?;
    output.emit(&after, &project.selected, &checkpoints::display(to))?;
    index::stop_observing(&before)?;
    index::stop_observing(&after)?;
    before.finish()?;
    after.finish()?;
    let summary = output.finish()?;
    Ok(summary)
}

fn changes(
    repository: &Connection,
    from: i64,
    to: i64,
    before: &Connection,
    after: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    crate::infrastructure::history::changes::prepare(repository, Some(from), to, cancel)?;
    for connection in [before, after] {
        connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS q_changed_inputs(path BLOB PRIMARY KEY); DELETE FROM q_changed_inputs;")?;
    }
    let mut statement = repository.prepare("SELECT path FROM snapshot_changes ORDER BY path")?;
    let mut rows = statement.query([])?;
    let mut left = before.prepare("INSERT INTO q_changed_inputs VALUES(?1)")?;
    let mut right = after.prepare("INSERT INTO q_changed_inputs VALUES(?1)")?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let path: Vec<u8> = row.get(0)?;
        left.execute([&path])?;
        right.execute([&path])?;
    }
    Ok(())
}
