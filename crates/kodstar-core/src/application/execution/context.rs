// Purpose: 명령의 프로젝트·lease·connection·초기 및 반복 동기화 수명.
// Pattern: Command Context.
// Usage: 공통 실행기가 준비하고 각 handler가 빌린다.
// Related: crates/kodstar-core/src/application/execution/mod.rs, crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/src/application/workspace/mod.rs.

use super::super::{
    analysis::{self, BuildOptions},
    engine::Engine,
    maintenance,
    workspace::{self, ProjectOptions, SelectedProject, Selection},
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, Result},
    query::Refresh,
    results::{BuildReport, MaintenanceReport, Progress, QueryMetadata},
};
use crate::infrastructure::{
    cache::{CacheStore, ProjectCache, now_seconds},
    index,
    watch::Changes,
};
use rusqlite::Connection;
use std::time::Instant;

#[derive(Clone, Copy)]
pub(crate) enum Plan<'a> {
    Build(&'a BuildOptions),
    Project(Refresh),
    Optional(Refresh),
    Watch,
    Detached,
}

pub(crate) struct ProjectContext {
    // Connections and observers close before the project lease is released.
    pub connection: Connection,
    pub changes: Option<Changes>,
    pub cache: ProjectCache,
    pub selected: SelectedProject,
    pub report: Option<BuildReport>,
    pub maintenance: Option<MaintenanceReport>,
    pub refresh: Refresh,
}

pub(crate) struct Context<'a> {
    pub store: &'a CacheStore,
    pub options: &'a ProjectOptions,
    pub project: Option<ProjectContext>,
    full: bool,
    build: bool,
    started: Instant,
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
            Plan::Build(options) => (
                Selection::Build(options.path.as_deref()),
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
            Plan::Optional(refresh) => (Selection::Existing, true, refresh, false, false),
            Plan::Watch => (Selection::Existing, false, Refresh::Metadata, false, true),
            Plan::Detached => (Selection::Existing, false, Refresh::None, false, false),
        };
        let build = matches!(selection, Selection::Build(_));
        let mut context = Self {
            store,
            options,
            project: None,
            full,
            build,
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
        let cache = store.acquire(&selected.root, build, cancel)?;
        let writer = if build || refresh != Refresh::None {
            Some(cache.writer(cancel)?)
        } else {
            None
        };
        if build {
            cache.initialize_record()?;
        }
        let database = cache.directory.join("index.sqlite");
        let connection = if build {
            index::open(
                &database,
                &selected.root,
                true,
                selected.settings.db_cache_bytes,
            )?
        } else if refresh != Refresh::None {
            index::open_existing_writer(
                &database,
                &selected.root,
                selected.settings.db_cache_bytes,
            )?
        } else {
            index::open(
                &database,
                &selected.root,
                false,
                selected.settings.db_cache_bytes,
            )?
        };
        if !build && let Err(error) = index::require_committed(&connection) {
            if optional && options.root.is_none() && error.kind == ErrorKind::NotFound {
                return Ok(context);
            }
            return Err(error);
        }
        let changes = if watch {
            Some(Changes::start(&selected.root, &store.home)?)
        } else {
            None
        };
        context.project = Some(ProjectContext {
            connection,
            changes,
            cache,
            selected,
            report: None,
            maintenance: None,
            refresh,
        });
        drop(writer);
        Ok(context)
    }

    pub(super) fn synchronize(
        &mut self,
        cancel: &Cancellation,
        progress: &mut dyn FnMut(&Progress) -> Result<()>,
    ) -> Result<()> {
        let Some(project) = &mut self.project else {
            return Ok(());
        };
        if self.build || project.refresh != Refresh::None {
            let _writer = project.cache.writer(cancel)?;
            let report = analysis::run(
                analysis::Session {
                    store: self.store,
                    selected: &project.selected,
                    cache: &project.cache,
                    started: self.started,
                },
                &mut project.connection,
                BuildOptions {
                    path: None,
                    full: self.full,
                    verify: project.refresh == Refresh::Verify,
                },
                cancel,
                progress,
            )?;
            project.maintenance = report.maintenance.clone();
            project.report = Some(report);
        } else {
            project.maintenance =
                maintenance::run(self.store, &project.selected, now_seconds()?, cancel)?;
        }
        project.cache.touch()?;
        self.build = false;
        self.full = false;
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
            maintenance: project.maintenance.clone(),
            schema_version: 1,
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
