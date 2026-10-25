// Purpose: 명령의 프로젝트·lease·connection·초기 및 반복 동기화 수명.
// Pattern: Command Context.
// Usage: 공통 실행기가 준비하고 각 handler가 빌린다.
// Related: crates/kodstar-core/src/application/execution/mod.rs, crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/src/application/workspace/mod.rs.

use super::super::{
    analysis::{self, InitOptions},
    engine::Engine,
    maintenance,
    workspace::{self, ProjectOptions, SelectedProject, Selection},
};
use super::state::{InputState, Intent};
use crate::application::history::capture;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, Result},
    query::Refresh,
    results::{InitReport, MaintenanceReport, Progress, QueryMetadata},
};
use crate::infrastructure::{
    cache::{CacheStore, ProjectCache, now_seconds},
    history::{Repository, checkpoints},
    index,
};
use rusqlite::Connection;
use std::time::Instant;

#[derive(Clone, Copy)]
pub(crate) enum Plan<'a> {
    Init(&'a InitOptions),
    Project(Refresh),
    Diagnostics(Refresh),
    Revision(&'a str, Refresh),
    Optional(Refresh),
    Watch,
    Detached,
}

pub(crate) struct ProjectContext {
    // Connections and observers close before the project lease is released.
    pub connection: Connection,
    pub database: std::path::PathBuf,
    pub(super) input: InputState,
    pub repository: Repository,
    pub cache: ProjectCache,
    pub selected: SelectedProject,
    pub report: Option<InitReport>,
    pub maintenance: Option<MaintenanceReport>,
    pub refresh: Refresh,
}

pub(crate) struct Context<'a> {
    pub store: &'a CacheStore,
    pub options: &'a ProjectOptions,
    pub project: Option<ProjectContext>,
    intent: Intent,
    started: Instant,
    // The process lease outlives project observers, connections and repository locks.
    _installation: Option<crate::infrastructure::cache::locks::FileLock>,
}

impl<'a> Context<'a> {
    pub(super) fn prepare(
        store: &'a CacheStore,
        options: &'a ProjectOptions,
        plan: Plan<'_>,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let started = Instant::now();
        let (selection, optional, refresh, full, watch) = match plan {
            Plan::Init(options) => (
                Selection::Init(options.path.as_deref()),
                false,
                if options.verify {
                    Refresh::Verify
                } else {
                    Refresh::Metadata
                },
                options.full,
                false,
            ),
            Plan::Project(refresh) => (Selection::Existing, false, refresh, false, false),
            Plan::Diagnostics(refresh) => (Selection::Existing, false, refresh, false, false),
            Plan::Revision(_, refresh) => (Selection::Existing, false, refresh, false, false),
            Plan::Optional(refresh) => (Selection::Existing, true, refresh, false, false),
            Plan::Watch => (Selection::Existing, false, Refresh::Metadata, false, true),
            Plan::Detached => (Selection::Existing, false, Refresh::None, false, false),
        };
        let initialize = matches!(selection, Selection::Init(_));
        let mut context = Self {
            _installation: crate::infrastructure::installation::lease(store, false, cancel)?,
            store,
            options,
            project: None,
            intent: if initialize {
                Intent::Initialize { full }
            } else if matches!(plan, Plan::Diagnostics(_)) {
                Intent::Diagnostics
            } else {
                Intent::Regular
            },
            started,
        };
        if matches!(plan, Plan::Detached) {
            return Ok(context);
        }
        let selected = match workspace::select(store, options, selection, cancel) {
            Ok(selected) => selected,
            Err(error)
                if optional && options.root.is_none() && error.kind == ErrorKind::NotFound =>
            {
                return Ok(context);
            }
            Err(error) => return Err(error),
        };
        if context._installation.is_none() && (initialize || Repository::exists(&selected.root)) {
            context._installation =
                crate::infrastructure::installation::lease(store, true, cancel)?;
        }
        context.project = Some(if let Plan::Revision(revision, refresh) = plan {
            super::prepare::revision(store, selected, revision, refresh, cancel)?
        } else {
            super::prepare::project(store, selected, initialize, refresh, watch, cancel)?
        });
        Ok(context)
    }

    pub(super) fn synchronize(
        &mut self,
        cancel: &Cancellation,
        progress: &mut dyn FnMut(&Progress) -> Result<()>,
    ) -> Result<()> {
        let result = self.synchronize_inner(cancel, progress);
        if let Some(project) = &mut self.project
            && project.input.watching()
        {
            project.repository.release();
        }
        result
    }

    fn synchronize_inner(
        &mut self,
        cancel: &Cancellation,
        progress: &mut dyn FnMut(&Progress) -> Result<()>,
    ) -> Result<()> {
        let Some(project) = &mut self.project else {
            return Ok(());
        };
        project.repository.acquire(cancel)?;
        if self.intent.initializes() || project.refresh != Refresh::None {
            if !project.input.historical() {
                let mut state = capture::run(
                    &mut project.repository,
                    &self.store.home,
                    &project.selected.settings,
                    project.refresh == Refresh::Verify,
                    cancel,
                )?;
                let id = checkpoints::head(&project.repository.connection)?.ok_or_else(|| {
                    Error::operation("initialization did not create a checkpoint")
                })?;
                project.selected.snapshot = Some(project.repository.snapshot(id));
                state.analysis_checkpoint = Some(state.checkpoint.clone());
                project.selected.history = Some(state);
            }
            let _writer = project.cache.writer(cancel)?;
            let outcome = analysis::run(
                analysis::Session {
                    store: self.store,
                    selected: &project.selected,
                    cache: &project.cache,
                    database: &project.database,
                    started: self.started,
                },
                &mut project.connection,
                InitOptions {
                    path: None,
                    full: self.intent.full(),
                    verify: project.refresh == Refresh::Verify,
                },
                cancel,
                progress,
            );
            let report = match outcome {
                Ok(report) => report,
                Err(error) if self.intent.diagnostic() => {
                    if let Some(state) = &mut project.selected.history {
                        state.analysis_error = Some(error.to_string());
                        state.analysis_checkpoint = project.connection.query_row(
                            "SELECT checkpoint FROM metadata",
                            [],
                            |r| r.get(0),
                        )?;
                    }
                    project.cache.touch()?;
                    return Ok(());
                }
                Err(error) => return Err(error),
            };
            project.maintenance = report.maintenance.clone();
            project.report = Some(report);
        } else {
            let mut state = if project.input.historical() {
                project
                    .selected
                    .history
                    .clone()
                    .ok_or_else(|| Error::operation("historical context has no checkpoint"))?
            } else {
                project.repository.state()?
            };
            let (checkpoint, error) = project.connection.query_row(
                "SELECT checkpoint,last_error FROM metadata WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            state.analysis_checkpoint = checkpoint;
            state.analysis_error = error;
            project.selected.history = Some(state);
            project.maintenance =
                maintenance::run(self.store, &project.selected, now_seconds()?, cancel)?;
        }
        project.cache.touch()?;
        self.intent.synchronized();
        self.started = Instant::now();
        Ok(())
    }

    pub(super) fn required(&mut self) -> Result<&mut ProjectContext> {
        self.project
            .as_mut()
            .ok_or_else(|| Error::operation("command requires a prepared project"))
    }

    pub(super) fn metadata(&self, cancel: &Cancellation) -> Result<QueryMetadata> {
        let Some(project) = &self.project else {
            return Ok(Engine::detached_metadata());
        };
        if let Some(report) = &project.report {
            return Ok(report.metadata(project.refresh));
        }
        cancel.check()?;
        index::limit_reads(&project.connection, project.selected.settings.memory_budget)?;
        index::observe_cancellation(&project.connection, cancel)?;
        let snapshot = project.connection.unchecked_transaction()?;
        let (generation, indexed_at) = index::require_committed(&snapshot)?;
        let coverage = index::coverage(&snapshot)?;
        let relation_registry_version = snapshot.query_row(
            "SELECT relation_registry_version FROM metadata WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        let metadata = QueryMetadata {
            history: project.selected.history.clone(),
            maintenance: project.maintenance.clone(),
            schema_version: 2,
            relation_registry_version,
            engine_version: crate::VERSION,
            root: Some(project.selected.root.to_string_lossy().into_owned()),
            root_encoding: crate::infrastructure::cache::paths::encoding(&project.selected.root),
            generation: Some(generation),
            indexed_at: Some(indexed_at),
            freshness: project.refresh.as_str(),
            unresolved_count: coverage.unresolved,
            coverage,
        };
        index::stop_observing(&snapshot)?;
        snapshot.finish()?;
        Ok(metadata)
    }
}
