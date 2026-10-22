// Purpose: 명령의 영구 이력·분석 캐시와 잠금 획득 순서.
// Pattern: Resource Scope.
// Usage: Context가 선택한 프로젝트의 실행 자원을 준비.
// Related: crates/kodstar-core/src/application/execution/context.rs.

use super::context::ProjectContext;
use crate::application::workspace::SelectedProject;
use crate::domain::{cancellation::Cancellation, diagnostics::Result, query::Refresh};
use crate::infrastructure::{cache::CacheStore, history::Repository, index, watch::Changes};

pub(super) fn project(
    store: &CacheStore,
    selected: SelectedProject,
    initialize: bool,
    refresh: Refresh,
    watch: bool,
    cancel: &Cancellation,
) -> Result<ProjectContext> {
    let repository = Repository::open(&selected.root, initialize, cancel)?;
    crate::infrastructure::installation::register(store, &selected.root, &repository.identity()?)?;
    let create = initialize || refresh != Refresh::None;
    let cache = store.acquire(&selected.root, create, cancel)?;
    let writer = if create {
        Some(cache.writer(cancel)?)
    } else {
        None
    };
    if create {
        cache.initialize_record()?;
    }
    let database = cache.directory.join("index.sqlite");
    let connection = index::open(
        &database,
        &selected.root,
        create,
        selected.settings.db_cache_bytes,
    )?;
    if !create {
        index::require_committed(&connection)?;
        index::require_history(&connection, &repository.identity()?, None)?;
    }
    let changes = if watch {
        Some(Changes::start(&selected.root, &store.home)?)
    } else {
        None
    };
    drop(writer);
    Ok(ProjectContext {
        connection,
        database,
        changes,
        repository,
        cache,
        selected,
        report: None,
        maintenance: None,
        refresh,
        revision: false,
        watching: watch,
    })
}

pub(super) fn revision(
    store: &CacheStore,
    mut selected: SelectedProject,
    revision: &str,
    refresh: Refresh,
    cancel: &Cancellation,
) -> Result<ProjectContext> {
    let repository = Repository::open(&selected.root, false, cancel)?;
    crate::infrastructure::installation::register(store, &selected.root, &repository.identity()?)?;
    selected.historical = true;
    let id =
        crate::infrastructure::history::checkpoints::resolve(&repository.connection, revision)?;
    let mut state = repository.state()?;
    state.checkpoint = crate::infrastructure::history::checkpoints::display(id);
    state.tracked_files = repository.connection.query_row(
        "SELECT tracked_files FROM checkpoints WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    state.analysis_checkpoint = Some(state.checkpoint.clone());
    selected.snapshot = Some(repository.snapshot(id));
    selected.history = Some(state);
    let create = refresh != Refresh::None;
    let cache = store.acquire(&selected.root, create, cancel)?;
    let writer = if create {
        Some(cache.writer(cancel)?)
    } else {
        None
    };
    if create {
        cache.initialize_record()?;
    }
    let database = cache.directory.join("past.sqlite");
    let connection = index::open(
        &database,
        &selected.root,
        create,
        selected.settings.db_cache_bytes,
    )?;
    if !create {
        index::require_committed(&connection)?;
        let checkpoint: Option<String> =
            connection.query_row("SELECT checkpoint FROM metadata", [], |r| r.get(0))?;
        if checkpoint.as_deref() != selected.history.as_ref().map(|s| s.checkpoint.as_str()) {
            return Err(crate::domain::diagnostics::Error::not_found(
                "no cached analysis for this checkpoint; retry without --no-refresh",
            ));
        }
        index::require_history(&connection, &repository.identity()?, checkpoint.as_deref())?;
    }
    drop(writer);
    Ok(ProjectContext {
        connection,
        database,
        changes: None,
        repository,
        cache,
        selected,
        report: None,
        maintenance: None,
        refresh,
        revision: true,
        watching: false,
    })
}
