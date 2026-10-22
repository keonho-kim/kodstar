// Purpose: 분석 성공과 독립적인 이력 명령 조정.
// Pattern: Use Case.
// Usage: 공통 명령 실행기가 history 준비 정책으로 호출.
// Related: crates/kodstar-core/src/application/history/compare.rs, crates/kodstar-core/src/application/history/branches.rs.

mod analysis_state;
mod branches;
pub(crate) mod capture;
mod compare;
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
        HistoryRequest::Diff { from, to, .. } if from.is_some() != to.is_some() => {
            Err(Error::argument("--from and --to must be supplied together"))
        }
        HistoryRequest::Diff {
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
    let selected = workspace::select(store, options, Selection::Existing, cancel)?;
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
    if let HistoryRequest::Diff { from, to, .. } = request {
        let to = to
            .as_deref()
            .map(|s| checkpoints::resolve(&repository.connection, s))
            .transpose()?
            .unwrap_or(head);
        let from = if let Some(from) = from {
            Some(checkpoints::resolve(&repository.connection, from)?)
        } else {
            repository.connection.query_row(
                "SELECT parent FROM checkpoints WHERE id=?1",
                [to],
                |r| r.get(0),
            )?
        };
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
    sink.begin(&metadata)?;
    let count = if let Some(operation) = operation {
        sink.row(&QueryRow::History(HistoryRow::Operation {
            operation: operation.into(),
            checkpoint: state.checkpoint,
            branch: state.branch,
            changed_files: state.changed_files,
        }))?;
        1
    } else {
        match request {
            HistoryRequest::List { .. } => listing::checkpoints(&repository, head, cancel, sink)?,
            HistoryRequest::Branches => listing::branches(&repository, cancel, sink)?,
            HistoryRequest::Diff { from, to, .. } => compare::run(
                &repository,
                head,
                from.as_deref(),
                to.as_deref(),
                selected.settings.max_file_bytes,
                cancel,
                sink,
            )?,
            _ => 0,
        }
    };
    let summary = QuerySummary {
        complete: true,
        result_count: count,
        ..QuerySummary::default()
    };
    sink.finish(&summary)?;
    Ok(summary)
}
