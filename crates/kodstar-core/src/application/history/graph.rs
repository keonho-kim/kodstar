// Purpose: 동일한 분석 환경에서 두 불변 체크포인트의 그래프를 수집하고 비교.
// Pattern: Use Case.
// Usage: history 실행기가 GraphDiff 요청과 해석한 체크포인트 ID를 전달.
// Related: crates/kodstar-core/src/application/history/graph/edges.rs, crates/kodstar-core/src/application/history/graph/snapshots.rs.

mod cache;
mod edges;
mod snapshots;

use crate::application::{analysis, engine::Engine, workspace::SelectedProject};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    history::Comparison,
    relations::RELATION_REGISTRY_VERSION,
    results::{QuerySink, QuerySummary},
};
use crate::infrastructure::{
    cache::{CacheStore, now_seconds, paths::encoding},
    history::{Repository, checkpoints},
    index,
};

pub(super) fn run(
    store: &CacheStore,
    repository: &mut Repository,
    mut project: SelectedProject,
    revisions: (Option<i64>, i64),
    limit: Option<usize>,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<QuerySummary> {
    let (from, to) = revisions;
    let configuration = analysis::fingerprint(&project.settings)?;
    let repository_id = repository.identity()?;
    let key = format!(
        "{repository_id}:{from:?}:{to}:{configuration}:{}:{RELATION_REGISTRY_VERSION}:{}:{}",
        index::EXTRACTION_VERSION,
        index::STORAGE_VERSION,
        project.settings.memory_budget
    );
    let cache = store.acquire(&project.root, true, cancel)?;
    project.historical = true;
    crate::application::maintenance::run(store, &project, now_seconds()?, cancel)?;
    let (mut connection, report) = {
        let _writer = cache.writer(cancel)?;
        cache.initialize_record()?;
        let build = snapshots::Build {
            store,
            cache: &cache,
            repository,
            repository_id: &repository_id,
            configuration: &configuration,
            cancel,
        };
        if from == Some(to) {
            let report = build.compare(&mut project, revisions, None, sink)?;
            (None, report)
        } else {
            let budget = project.settings.memory_budget;
            let saved = cache::open(&cache.directory, budget, cancel)?;
            if let Some(report) = cache::read(&saved, &key)? {
                (Some(saved), report)
            } else {
                let mut spool = edges::Spool::new(saved, budget)?;
                let report = build.compare(&mut project, revisions, Some(&mut spool), sink)?;
                (Some(spool.publish(&key, &report)?), report)
            }
        }
    };
    cache.touch()?;
    snapshots::attach(&mut project, repository, to)?;
    let pinned = if let Some(connection) = &mut connection {
        let transaction = connection.transaction()?;
        let stored: String = transaction.query_row(
            "SELECT key FROM graph_metadata WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if stored != key {
            return Err(Error::operation("graph comparison cache was replaced"));
        }
        Some(transaction)
    } else {
        None
    };
    repository.release();
    let mut metadata = Engine::detached_metadata();
    metadata.root = Some(project.root.to_string_lossy().into_owned());
    metadata.root_encoding = encoding(&project.root);
    metadata.freshness = "checkpoint";
    metadata.generation = Some(report.generation);
    metadata.indexed_at = Some(report.indexed_at);
    metadata.coverage = report.after.clone();
    metadata.unresolved_count = report.after.unresolved;
    metadata.history = project.history.take();
    if let Some(state) = &mut metadata.history {
        state.comparison = Some(Comparison {
            from: from.map(checkpoints::display),
            to: checkpoints::display(to),
            before_coverage: report.before.clone(),
            after_coverage: Some(report.after.clone()),
            configuration: Some(configuration),
            extraction_version: Some(index::EXTRACTION_VERSION),
        });
    }
    sink.begin(&metadata)?;
    let mut summary = QuerySummary {
        complete: true,
        unresolved_count: report.after.unresolved,
        ..QuerySummary::default()
    };
    if let Some(previous) = &report.before {
        summary.include_coverage(previous);
    }
    summary.include_coverage(&report.after);
    if let Some(pinned) = pinned {
        let maximum = match limit {
            None => Some(project.settings.limit),
            Some(0) => None,
            Some(value) => Some(value),
        };
        edges::emit(&pinned, maximum, cancel, sink, &mut summary)?;
        pinned.finish()?;
    }
    sink.finish(&summary)?;
    Ok(summary)
}
