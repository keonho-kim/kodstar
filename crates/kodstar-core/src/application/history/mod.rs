// Purpose: 로컬 이력 요청의 체크포인트 선택과 실행 조정.
// Pattern: Use Case.
// Usage: 공통 명령 실행기가 history 준비 정책으로 호출.
// Related: crates/kodstar-core/src/application/history/compare.rs, crates/kodstar-core/src/application/history/graph.rs, crates/kodstar-core/src/application/history/branches.rs.

mod analysis_state;
mod branches;
pub(crate) mod capture;
mod compare;
mod graph;
pub(crate) mod impact;
mod listing;
mod text;

use crate::application::{
    engine::Engine,
    workspace::{self, ProjectOptions, Selection},
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    history::{HistoryRequest, HistoryRow},
    query::Refresh,
    results::{QueryRow, QuerySink, QuerySummary},
};
use crate::infrastructure::{
    cache::{CacheStore, paths::encoding},
    history::{Repository, checkout, checkpoints},
};

pub(crate) fn validate(request: &HistoryRequest) -> Result<()> {
    match request {
        HistoryRequest::Save { name }
        | HistoryRequest::CreateBranch { name }
        | HistoryRequest::SwitchBranch { name }
        | HistoryRequest::DeleteBranch { name } => crate::domain::history::validate_name(name),
        HistoryRequest::Diff { from, to, .. } | HistoryRequest::GraphDiff { from, to, .. }
            if from.is_some() != to.is_some() =>
        {
            Err(Error::argument("--from and --to must be supplied together"))
        }
        HistoryRequest::Diff {
            from: Some(_),
            refresh: Refresh::Verify,
            ..
        }
        | HistoryRequest::GraphDiff {
            from: Some(_),
            refresh: Refresh::Verify,
            ..
        } => Err(Error::argument(
            "fixed checkpoint comparison does not accept --verify",
        )),
        _ => Ok(()),
    }
}

pub(crate) fn run(
    store: &CacheStore,
    options: &ProjectOptions,
    request: &HistoryRequest,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<QuerySummary> {
    validate(request)?;
    let mut selected = workspace::select(store, options, Selection::Existing, cancel)?;
    let _installation = if !store.home.exists() && Repository::exists(&selected.root) {
        crate::infrastructure::installation::lease(store, true, cancel)?
    } else {
        None
    };
    let mut repository = if matches!(request, HistoryRequest::Upgrade) {
        Repository::upgrade(&selected.root, cancel)?
    } else {
        Repository::open(&selected.root, false, cancel)?
    };
    crate::infrastructure::installation::register(store, &selected.root, &repository.identity()?)?;
    let refresh = match request {
        HistoryRequest::List { refresh }
        | HistoryRequest::Diff {
            from: None,
            refresh,
            ..
        }
        | HistoryRequest::GraphDiff {
            from: None,
            refresh,
            ..
        } => *refresh,
        HistoryRequest::Save { .. }
        | HistoryRequest::Restore { .. }
        | HistoryRequest::CreateBranch { .. }
        | HistoryRequest::SwitchBranch { .. } => Refresh::Verify,
        _ => Refresh::None,
    };
    let mut state = if refresh != Refresh::None {
        capture::run(
            &mut repository,
            &store.home,
            &selected.settings,
            refresh == Refresh::Verify,
            cancel,
        )?
    } else {
        repository.state()?
    };
    let head = checkpoints::head(&repository.connection)?
        .ok_or_else(|| Error::not_found("initial checkpoint is missing; run kodstar init"))?;
    let mut operation = None;
    let mut restored = None;
    match request {
        HistoryRequest::Upgrade => operation = Some("upgrade"),
        HistoryRequest::Save { name } => {
            checkpoints::label(&repository.connection, head, name)?;
            operation = Some("save");
        }
        HistoryRequest::Restore { revision, path } => {
            let target = checkpoints::resolve(&repository.connection, revision)?;
            let path = path
                .as_ref()
                .map(|p| {
                    crate::infrastructure::history::paths::from_input(
                        &selected.root,
                        &selected.cwd,
                        p,
                    )
                })
                .transpose()?;
            restored = Some(checkout::start(
                &repository,
                target,
                None,
                path.as_deref(),
                cancel,
            )?);
            operation = Some("restore");
        }
        HistoryRequest::CreateBranch { .. }
        | HistoryRequest::SwitchBranch { .. }
        | HistoryRequest::DeleteBranch { .. } => {
            branches::run(&repository, request, head, cancel)?;
            operation = Some("branch");
        }
        _ => {}
    }
    if operation.is_some() {
        let created = state.created;
        let changes = state.changed_files;
        state = repository.state()?;
        state.created = created;
        state.changed_files = restored.unwrap_or(changes);
        if restored.is_some_and(|n| n > 0) {
            state.created = true;
        }
        if matches!(request, HistoryRequest::SwitchBranch { .. }) {
            state.created = false;
        }
    }
    let comparison = match request {
        HistoryRequest::Diff { from, to, .. } | HistoryRequest::GraphDiff { from, to, .. } => Some(
            compare::checkpoints(&repository, head, from.as_deref(), to.as_deref())?,
        ),
        _ => None,
    };
    if let Some((from, to)) = comparison {
        if let HistoryRequest::GraphDiff { limit, .. } = request {
            selected.history = Some(state);
            return graph::run(
                store,
                &mut repository,
                selected,
                (from, to),
                *limit,
                cancel,
                sink,
            );
        }
        state.comparison = Some(crate::domain::history::Comparison {
            before_coverage: None,
            after_coverage: None,
            configuration: None,
            extraction_version: None,
            from: from.map(checkpoints::display),
            to: checkpoints::display(to),
        });
    }
    let mut metadata = Engine::detached_metadata();
    analysis_state::read(store, &selected.root, &mut state, cancel);
    metadata.root = Some(selected.root.to_string_lossy().into_owned());
    metadata.root_encoding = encoding(&selected.root);
    metadata.freshness = refresh.as_str();
    metadata.history = Some(state.clone());
    let count = if let Some(operation) = operation {
        repository.release();
        sink.begin(&metadata)?;
        sink.row(&QueryRow::History(HistoryRow::Operation {
            operation: operation.into(),
            checkpoint: state.checkpoint,
            branch: state.branch,
            changed_files: state.changed_files,
        }))?;
        1
    } else {
        let snapshot = repository.query_snapshot()?;
        sink.begin(&metadata)?;
        let count = match request {
            HistoryRequest::List { .. } => listing::checkpoints(&snapshot, head, cancel, sink)?,
            HistoryRequest::Branches => listing::branches(&snapshot, cancel, sink)?,
            HistoryRequest::Diff { .. } => compare::run(
                &snapshot,
                comparison
                    .ok_or_else(|| Error::operation("missing source comparison checkpoints"))?,
                selected.settings.max_file_bytes,
                cancel,
                sink,
            )?,
            _ => 0,
        };
        snapshot.finish()?;
        count
    };
    let summary = QuerySummary {
        complete: true,
        result_count: count,
        ..QuerySummary::default()
    };
    sink.finish(&summary)?;
    Ok(summary)
}
