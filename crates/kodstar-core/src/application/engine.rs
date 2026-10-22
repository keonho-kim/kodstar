// Purpose: 공개 엔진 API에서 공통 명령 실행기로 연결.
// Pattern: Composition Root.
// Usage: core 공개 API 소비자가 Engine 메서드 호출.
// Related: crates/kodstar-core/src/application/execution/mod.rs, crates/kodstar-core/src/application/execution/reads.rs.

use std::path::PathBuf;

use super::analysis::InitOptions;
use super::execution::{self, reads};
use super::workspace::ProjectOptions;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::query::{Query, QueryOptions, Refresh};
use crate::domain::results::{
    InitReport, LanguageCapability, Progress, QueryMetadata, QuerySink, QuerySummary,
};
use crate::infrastructure::cache::CacheStore;
use crate::infrastructure::languages;

/// An engine handle contains configuration paths, not a resident source graph or parser pool.
#[derive(Clone, Debug)]
pub struct Engine {
    pub(crate) cache: CacheStore,
}

impl Engine {
    pub fn history(
        &self,
        project: &ProjectOptions,
        request: &crate::domain::history::HistoryRequest,
        cancel: &Cancellation,
        sink: &mut dyn QuerySink,
    ) -> Result<QuerySummary> {
        execution::run(
            &self.cache,
            project,
            execution::history::History { request, sink },
            cancel,
        )
    }
    pub fn new(cache_home: PathBuf) -> Result<Self> {
        if !cache_home.is_absolute() {
            return Err(Error::argument("cache home must be an absolute path"));
        }
        Ok(Self {
            cache: CacheStore { home: cache_home },
        })
    }

    pub fn from_environment() -> Result<Self> {
        let home = std::env::var_os("HOME");
        let home = home.ok_or_else(|| Error::operation("user home directory is unavailable"))?;
        Self::new(PathBuf::from(home).join(".kodstar"))
    }

    pub fn init(
        &self,
        project: &ProjectOptions,
        options: InitOptions,
        cancel: &Cancellation,
        progress: &mut dyn FnMut(&Progress) -> Result<()>,
    ) -> Result<InitReport> {
        execution::run(
            &self.cache,
            project,
            execution::Init {
                options: &options,
                progress,
            },
            cancel,
        )
    }

    pub fn query(
        &self,
        project: &ProjectOptions,
        query: &Query,
        options: &QueryOptions,
        cancel: &Cancellation,
        sink: &mut dyn QuerySink,
    ) -> Result<QuerySummary> {
        execution::run(
            &self.cache,
            project,
            reads::Query {
                query,
                options,
                sink,
            },
            cancel,
        )
    }

    pub fn status(
        &self,
        project: &ProjectOptions,
        doctor: bool,
        cancel: &Cancellation,
        sink: &mut dyn QuerySink,
    ) -> Result<QuerySummary> {
        execution::run(
            &self.cache,
            project,
            reads::Status {
                doctor,
                refresh: Refresh::Metadata,
                sink,
            },
            cancel,
        )
    }

    pub fn cache(
        &self,
        project: &ProjectOptions,
        request: super::cache::CacheRequest,
        cancel: &Cancellation,
        sink: &mut dyn QuerySink,
    ) -> Result<QuerySummary> {
        let refresh = if matches!(request, super::cache::CacheRequest::List) {
            Refresh::Metadata
        } else {
            Refresh::None
        };
        execution::run(
            &self.cache,
            project,
            execution::Cache {
                request,
                refresh,
                sink,
            },
            cancel,
        )
    }

    pub fn watch(
        &self,
        project: &ProjectOptions,
        cancel: &Cancellation,
        sink: &mut dyn QuerySink,
    ) -> Result<QuerySummary> {
        execution::run(&self.cache, project, execution::Watch { sink }, cancel)
    }

    pub fn capabilities() -> impl Iterator<Item = LanguageCapability> {
        crate::domain::languages::Language::ALL
            .into_iter()
            .map(languages::capability)
    }

    pub fn language_report(
        &self,
        project: &ProjectOptions,
        cancel: &Cancellation,
        sink: &mut dyn QuerySink,
    ) -> Result<QuerySummary> {
        execution::run(
            &self.cache,
            project,
            reads::Languages {
                refresh: Refresh::Metadata,
                sink,
            },
            cancel,
        )
    }

    pub(crate) fn detached_metadata() -> QueryMetadata {
        QueryMetadata {
            history: None,
            maintenance: None,
            schema_version: 2,
            relation_registry_version: crate::domain::relations::RELATION_REGISTRY_VERSION,
            engine_version: crate::VERSION,
            root: None,
            root_encoding: None,
            generation: None,
            indexed_at: None,
            freshness: "not_checked",
            coverage: Default::default(),
            unresolved_count: 0,
        }
    }
}
