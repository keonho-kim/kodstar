// Purpose: 공개 명령 요청과 init·cache·watch의 공통 실행 연결.
// Pattern: Command Handler.
// Usage: CLI·stdio·Rust API가 Engine::execute 또는 편의 메서드를 호출.
// Related: crates/kodstar-core/src/application/execution/mod.rs, crates/kodstar-core/src/application/execution/reads.rs, crates/kodstar-core/src/application/engine.rs.

use super::super::{
    analysis::InitOptions,
    cache::{self, CacheRequest},
    engine::Engine,
    workspace::ProjectOptions,
};
use super::{
    CommandHandler,
    context::{Context, Plan},
    reads,
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    query::{Query, QueryOptions, Refresh},
    results::{InitReport, Progress, QueryRow, QuerySink, QuerySummary},
};

/// One engine command. Output formatting and process/session control remain with the caller.
pub enum Command<'a> {
    History(&'a crate::domain::history::HistoryRequest),
    Init(&'a InitOptions),
    Query {
        query: &'a Query,
        options: &'a QueryOptions,
    },
    Status {
        doctor: bool,
        refresh: Refresh,
    },
    Languages {
        refresh: Refresh,
    },
    Cache {
        request: CacheRequest,
        refresh: Refresh,
    },
    Watch,
}

impl Engine {
    pub fn execute(
        &self,
        project: &ProjectOptions,
        command: Command<'_>,
        cancel: &Cancellation,
        sink: &mut dyn QuerySink,
    ) -> Result<QuerySummary> {
        match command {
            Command::History(request) => self.history(project, request, cancel, sink),
            Command::Init(options) => {
                let report = self.init(project, options.clone(), cancel, &mut |event| {
                    sink.progress(event)
                })?;
                let refresh = if options.verify {
                    Refresh::Verify
                } else {
                    Refresh::Metadata
                };
                sink.begin(&report.metadata(refresh))?;
                let summary = QuerySummary {
                    complete: true,
                    result_count: 1,
                    unresolved_count: report.coverage.unresolved,
                    ..QuerySummary::default()
                };
                sink.row(&QueryRow::Init(report))?;
                sink.finish(&summary)?;
                Ok(summary)
            }
            Command::Query { query, options } => self.query(project, query, options, cancel, sink),
            Command::Status { doctor, refresh } => super::run(
                &self.cache,
                project,
                reads::Status {
                    doctor,
                    refresh,
                    sink,
                },
                cancel,
            ),
            Command::Languages { refresh } => super::run(
                &self.cache,
                project,
                reads::Languages { refresh, sink },
                cancel,
            ),
            Command::Cache { request, refresh } => super::run(
                &self.cache,
                project,
                Cache {
                    request,
                    refresh,
                    sink,
                },
                cancel,
            ),
            Command::Watch => self.watch(project, cancel, sink),
        }
    }
}

pub(crate) struct Init<'a> {
    pub options: &'a InitOptions,
    pub progress: &'a mut dyn FnMut(&Progress) -> Result<()>,
}
impl CommandHandler for Init<'_> {
    type Output = InitReport;
    fn plan(&self) -> Plan<'_> {
        Plan::Init(self.options)
    }
    fn progress(&mut self, event: &Progress) -> Result<()> {
        (self.progress)(event)
    }
    fn execute(self, context: &mut Context<'_>, _: &Cancellation) -> Result<InitReport> {
        context
            .required()?
            .report
            .take()
            .ok_or_else(|| Error::operation("initialization did not publish a report"))
    }
}

pub(crate) struct Cache<'a> {
    pub request: CacheRequest,
    pub refresh: Refresh,
    pub sink: &'a mut dyn QuerySink,
}
impl CommandHandler for Cache<'_> {
    type Output = QuerySummary;
    fn validate(&self) -> Result<()> {
        if !matches!(self.request, CacheRequest::List) && self.refresh != Refresh::None {
            return Err(Error::argument(
                "cache recovery/cleanup does not accept refresh options",
            ));
        }
        Ok(())
    }
    fn plan(&self) -> Plan<'_> {
        if matches!(self.request, CacheRequest::List) {
            Plan::Optional(self.refresh)
        } else {
            Plan::Detached
        }
    }
    fn progress(&mut self, event: &Progress) -> Result<()> {
        self.sink.progress(event)
    }
    fn execute(self, context: &mut Context<'_>, cancel: &Cancellation) -> Result<QuerySummary> {
        let metadata = context.metadata(cancel)?;
        cache::run(
            context.store,
            context.options,
            self.request,
            cancel,
            self.sink,
            context.project.as_ref().map(|project| &project.selected),
            metadata,
        )
    }
}

pub(crate) struct Watch<'a> {
    pub sink: &'a mut dyn QuerySink,
}
impl CommandHandler for Watch<'_> {
    type Output = QuerySummary;
    fn plan(&self) -> Plan<'_> {
        Plan::Watch
    }
    fn progress(&mut self, event: &Progress) -> Result<()> {
        self.sink.progress(event)
    }
    fn execute(self, context: &mut Context<'_>, cancel: &Cancellation) -> Result<QuerySummary> {
        super::watch::run(context, cancel, self.sink)
    }
}
