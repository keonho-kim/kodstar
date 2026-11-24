// Purpose: 준비된 프로젝트에서 query·status·languages의 snapshot과 결과 전송.
// Pattern: Command Handler.
// Usage: 공통 실행기가 동기화한 뒤 읽기 handler를 호출.
// Related: crates/kodstar-core/src/application/execution/context.rs, crates/kodstar-core/src/application/query/mod.rs, crates/kodstar-core/src/infrastructure/index/mod.rs.

use super::super::{engine::Engine, query};
use super::{
    CommandHandler,
    context::{Context, Plan},
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Diagnostic, Error, Result},
    query::{Query as Request, QueryOptions, Refresh},
    results::{Progress, QueryMetadata, QueryRow, QuerySink, QuerySummary, Status as StatusRow},
};
use crate::infrastructure::{cache::paths::encoding, index};

pub(crate) struct Query<'a> {
    pub query: &'a Request,
    pub options: &'a QueryOptions,
    pub sink: &'a mut dyn QuerySink,
}
impl CommandHandler for Query<'_> {
    type Output = QuerySummary;
    fn validate(&self) -> Result<()> {
        self.options.validate(self.query)
    }
    fn plan(&self) -> Plan<'_> {
        self.options
            .revision
            .as_deref()
            .map_or(Plan::Project(self.options.refresh), |revision| {
                Plan::Revision(revision, self.options.refresh)
            })
    }
    fn progress(&mut self, event: &Progress) -> Result<()> {
        self.sink.progress(event)
    }
    fn execute(self, context: &mut Context<'_>, cancel: &Cancellation) -> Result<QuerySummary> {
        let store = context.store;
        let project = context.required()?;
        if let Request::Changes { since } = self.query {
            return crate::application::history::impact::run(
                store,
                project,
                since,
                self.options,
                cancel,
                self.sink,
            );
        }
        index::limit_reads(&project.connection, project.selected.settings.memory_budget)?;
        index::observe_cancellation(&project.connection, cancel)?;
        let snapshot = project.connection.transaction()?;
        if matches!(self.query, Request::Explain { .. }) && project.selected.snapshot.is_none() {
            let checkpoint: Option<String> =
                snapshot.query_row("SELECT checkpoint FROM metadata", [], |row| row.get(0))?;
            let id = crate::infrastructure::history::checkpoints::resolve(
                &project.repository.connection,
                checkpoint
                    .as_deref()
                    .ok_or_else(|| Error::operation("analysis has no checkpoint"))?,
            )?;
            project.selected.snapshot = Some(project.repository.snapshot(id));
        }
        let result = query::run(
            &snapshot,
            query::ReadScope {
                selected: &project.selected,
                maintenance: project.maintenance.clone(),
                repository: Some(&mut project.repository),
            },
            self.query,
            self.options,
            cancel,
            self.sink,
        );
        index::stop_observing(&snapshot)?;
        snapshot.finish()?;
        result
    }
}

pub(crate) struct Status<'a> {
    pub doctor: bool,
    pub refresh: Refresh,
    pub sink: &'a mut dyn QuerySink,
}
impl CommandHandler for Status<'_> {
    type Output = QuerySummary;
    fn plan(&self) -> Plan<'_> {
        Plan::Diagnostics(self.refresh)
    }
    fn progress(&mut self, event: &Progress) -> Result<()> {
        self.sink.progress(event)
    }
    fn execute(self, context: &mut Context<'_>, cancel: &Cancellation) -> Result<QuerySummary> {
        let project = context.required()?;
        index::limit_reads(&project.connection, project.selected.settings.memory_budget)?;
        index::observe_cancellation(&project.connection, cancel)?;
        let snapshot = project.connection.transaction()?;
        let (generation,indexed_at,last_update,last_error,relation_registry_version): (i64,u64,String,Option<String>,u32) = snapshot.query_row(
            "SELECT generation,indexed_at,last_update,last_error,relation_registry_version FROM metadata WHERE singleton=1", [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))?;
        let coverage = index::coverage(&snapshot)?;
        let integrity = if self.doctor {
            project.repository.verify(cancel)?;
            let check: String = snapshot.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
            if check != "ok" {
                return Err(Error::operation(format!(
                    "index integrity check failed: {check}"
                )));
            }
            let mut keys = snapshot.prepare("PRAGMA foreign_key_check")?;
            if keys.query([])?.next()?.is_some() {
                return Err(Error::operation("index foreign-key check failed"));
            }
            index::dictionary::verify(&snapshot)?;
            index::atoms::verify(&snapshot)?;
            index::dependencies::verify(&snapshot)?;
            index::relations::verify(&snapshot)?;
            Some(check)
        } else {
            None
        };
        project.repository.release();
        self.sink.begin(&QueryMetadata {
            history: project.selected.history.clone(),
            maintenance: project.maintenance.clone(),
            schema_version: 2,
            relation_registry_version,
            engine_version: crate::VERSION,
            root: Some(project.selected.root.to_string_lossy().into_owned()),
            root_encoding: encoding(&project.selected.root),
            generation: Some(generation),
            indexed_at: Some(indexed_at),
            freshness: project.refresh.as_str(),
            unresolved_count: coverage.unresolved,
            coverage: coverage.clone(),
        })?;
        let published = generation > 0 && last_error.is_none();
        self.sink.row(&QueryRow::Status(StatusRow {
            history: project.selected.history.clone(),
            maintenance: project.maintenance.clone(),
            root: project.selected.root.to_string_lossy().into_owned(),
            cache: project.cache.directory.to_string_lossy().into_owned(),
            generation,
            indexed_at,
            last_update,
            last_error,
            coverage: coverage.clone(),
            storage_version: index::STORAGE_VERSION,
            integrity,
        }))?;
        let mut count = 1;
        if self.doctor {
            let mut statement = snapshot.prepare("SELECT d.code,d.message,f.display_path,d.line FROM diagnostics d LEFT JOIN files f ON f.id=d.file_id ORDER BY f.path,d.id")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                cancel.check()?;
                self.sink.row(&QueryRow::Diagnostic(Diagnostic {
                    code: row.get(0)?,
                    message: row.get(1)?,
                    file: row.get(2)?,
                    line: row.get(3)?,
                }))?;
                count += 1;
            }
        }
        let mut summary = QuerySummary {
            complete: true,
            result_count: count,
            ..QuerySummary::default()
        };
        summary.include_coverage(&coverage);
        if !published {
            summary.complete = false;
            summary.reasons.push("analysis_unpublished");
        }
        self.sink.finish(&summary)?;
        index::stop_observing(&snapshot)?;
        snapshot.finish()?;
        Ok(summary)
    }
}

pub(crate) struct Languages<'a> {
    pub refresh: Refresh,
    pub sink: &'a mut dyn QuerySink,
}
impl CommandHandler for Languages<'_> {
    type Output = QuerySummary;
    fn plan(&self) -> Plan<'_> {
        Plan::Optional(self.refresh)
    }
    fn progress(&mut self, event: &Progress) -> Result<()> {
        self.sink.progress(event)
    }
    fn execute(self, context: &mut Context<'_>, cancel: &Cancellation) -> Result<QuerySummary> {
        self.sink.begin(&context.metadata(cancel)?)?;
        let mut count = 0;
        for language in Engine::capabilities() {
            self.sink.row(&QueryRow::Language(language))?;
            count += 1;
        }
        let summary = QuerySummary {
            complete: true,
            result_count: count,
            ..QuerySummary::default()
        };
        self.sink.finish(&summary)?;
        Ok(summary)
    }
}
