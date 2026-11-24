// Purpose: 구조화된 엔진 결과와 스트리밍 sink 계약.
// Pattern: Simple Module.
// Usage: Engine이 호출자가 제공한 sink에 전달.
// Related: crates/kodstar-core/src/application/engine.rs.

//! Streaming engine results. Presentation and transport framing belong to the caller.

use super::diagnostics::{Coverage, Diagnostic, Result};
use super::source::{PathEncoding, SourceSpan};
use super::symbols::Symbol;
use std::sync::Arc;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceReport {
    pub last_run_at: u64,
    pub status: String,
    pub complete: bool,
    pub visited: u64,
    pub removed: u64,
    pub remaining_bytes: u64,
    pub size_complete: bool,
    pub reason: Option<String>,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct QueryMetadata {
    pub history: Option<super::history::HistoryState>,
    pub maintenance: Option<MaintenanceReport>,
    pub schema_version: u32,
    pub relation_registry_version: u32,
    pub engine_version: &'static str,
    pub root: Option<String>,
    pub root_encoding: Option<PathEncoding>,
    pub generation: Option<i64>,
    pub indexed_at: Option<u64>,
    pub freshness: &'static str,
    pub coverage: Coverage,
    pub unresolved_count: u64,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct QuerySummary {
    pub complete: bool,
    pub truncated: bool,
    pub result_count: u64,
    pub visited_count: u64,
    pub unresolved_count: u64,
    pub reasons: Vec<&'static str>,
}

impl QuerySummary {
    pub(crate) fn truncate(&mut self, reason: &'static str) {
        self.complete = false;
        self.truncated = true;
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }

    /// Input omissions make graph answers partial, independently of query row limits.
    pub(crate) fn include_coverage(&mut self, coverage: &Coverage) {
        for (omitted, reason) in [
            (coverage.partial_files > 0, "source_syntax_partial"),
            (coverage.fragment_files > 0, "source_fragment_context"),
            (
                coverage.configuration_incomplete_files > 0,
                "configuration_incomplete",
            ),
            (coverage.failed_files > 0, "source_analysis_failed"),
            (
                coverage.skipped_files > coverage.failed_files,
                "source_input_limit",
            ),
        ] {
            if omitted {
                self.complete = false;
                if !self.reasons.contains(&reason) {
                    self.reasons.push(reason);
                }
            }
        }
    }
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct UseSite {
    pub file: String,
    pub span: SourceSpan,
    pub path_encoding: Option<PathEncoding>,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct DocumentationReference {
    pub kind: &'static str,
    pub document_kind: &'static str,
    pub reference: String,
    pub source: UseSite,
    pub target: Option<UseSite>,
    pub resolution: &'static str,
    pub reason: Option<&'static str>,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct Relationship {
    pub source: Arc<Symbol>,
    pub target: Option<Arc<Symbol>>,
    pub external: Option<String>,
    pub relation: String,
    pub expression: String,
    pub direction: &'static str,
    pub resolution: String,
    pub rule: String,
    pub reason: Option<String>,
    pub site: UseSite,
    pub count: u64,
    pub depth: usize,
    pub condition: Option<String>,
    pub type_only: bool,
    pub provider: &'static str,
    pub provider_version: &'static str,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct FileConnection {
    pub file: String,
    pub path_encoding: Option<PathEncoding>,
    pub direction: &'static str,
    pub relation: String,
    pub count: u64,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct OverviewPath {
    pub path: String,
    pub path_encoding: Option<PathEncoding>,
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverviewRow {
    Module {
        path: String,
        path_encoding: Option<PathEncoding>,
        files: u64,
    },
    ModuleDependency {
        from: String,
        from_encoding: Option<PathEncoding>,
        to: String,
        to_encoding: Option<PathEncoding>,
        relation: String,
        count: u64,
        site: UseSite,
    },
    PackageDependency {
        from_package: String,
        to_package: String,
        from: OverviewPath,
        to: OverviewPath,
        relation: String,
        count: u64,
        site: UseSite,
    },
    Hub {
        symbol: Box<Symbol>,
        incoming: u64,
    },
    ImportCycle {
        files: usize,
        sample: Vec<OverviewPath>,
    },
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct LanguageCapability {
    pub language: &'static str,
    pub input_kind: &'static str,
    pub extensions: &'static [&'static str],
    pub input_recognition: bool,
    pub definition_extraction: bool,
    pub file_relationships: bool,
    pub direct_relationships: bool,
    pub semantic_enrichment: bool,
    pub provider: &'static str,
    pub limitations: Vec<&'static str>,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct Status {
    pub history: Option<super::history::HistoryState>,
    pub maintenance: Option<MaintenanceReport>,
    pub root: String,
    pub cache: String,
    pub generation: i64,
    pub indexed_at: u64,
    pub last_update: String,
    pub last_error: Option<String>,
    pub coverage: Coverage,
    pub storage_version: u32,
    pub integrity: Option<String>,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct CacheEntry {
    pub root: String,
    pub root_encoding: Option<PathEncoding>,
    pub cache_encoding: Option<PathEncoding>,
    pub bytes_complete: bool,
    pub cache: String,
    pub bytes: u64,
    pub last_used: u64,
    pub active: bool,
    pub action: &'static str,
    pub reason: Option<&'static str>,
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum QueryRow {
    History(super::history::HistoryRow),
    Init(InitReport),
    Symbol(Symbol),
    Relationship(Box<Relationship>),
    File(FileConnection),
    Documentation(Box<DocumentationReference>),
    Overview(Box<OverviewRow>),
    Language(LanguageCapability),
    Status(Status),
    Cache(CacheEntry),
    Diagnostic(Diagnostic),
}

/// A transport boundary; callbacks consume each row before the engine reads the next one.
pub trait QuerySink {
    fn begin(&mut self, metadata: &QueryMetadata) -> Result<()>;
    fn row(&mut self, row: &QueryRow) -> Result<()>;
    fn finish(&mut self, summary: &QuerySummary) -> Result<()>;

    fn progress(&mut self, _progress: &Progress) -> Result<()> {
        Ok(())
    }
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct Progress {
    pub phase: &'static str,
    pub completed_files: u64,
    pub source_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
impl Progress {
    pub fn phase(phase: &'static str) -> Self {
        Self {
            phase,
            completed_files: 0,
            source_bytes: 0,
            current_task: None,
            detail: None,
        }
    }
    pub fn task(phase: &'static str, task: String, detail: Option<String>) -> Self {
        Self {
            current_task: Some(task),
            detail,
            ..Self::phase(phase)
        }
    }
    pub(crate) fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.current_task.as_ref().map_or(0, String::capacity)
            + self.detail.as_ref().map_or(0, String::capacity)
    }
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct InitReport {
    pub history: Option<super::history::HistoryState>,
    pub maintenance: Option<MaintenanceReport>,
    pub root: String,
    pub root_encoding: Option<PathEncoding>,
    pub cache_encoding: Option<PathEncoding>,
    pub cache: String,
    pub generation: i64,
    pub indexed_at: u64,
    pub changed: bool,
    pub parsed_files: u64,
    pub deleted_files: u64,
    pub workers: usize,
    pub duration_ms: u64,
    pub coverage: Coverage,
}

impl InitReport {
    /// Metadata for this completed report; the caller declares which source verification was requested.
    pub fn metadata(&self, freshness: super::query::Refresh) -> QueryMetadata {
        QueryMetadata {
            history: self.history.clone(),
            maintenance: self.maintenance.clone(),
            schema_version: 2,
            relation_registry_version: super::relations::RELATION_REGISTRY_VERSION,
            engine_version: crate::VERSION,
            root: Some(self.root.clone()),
            root_encoding: self.root_encoding.clone(),
            generation: Some(self.generation),
            indexed_at: Some(self.indexed_at),
            freshness: freshness.as_str(),
            unresolved_count: self.coverage.unresolved,
            coverage: self.coverage.clone(),
        }
    }
}
