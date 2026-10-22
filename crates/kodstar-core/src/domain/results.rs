// Purpose: 구조화된 엔진 결과와 스트리밍 sink 계약.
// Pattern: Simple Module.
// Usage: Engine이 호출자가 제공한 sink에 전달.
// Related: crates/kodstar-core/src/application/engine.rs.

//! Streaming engine results. Presentation and transport framing belong to the caller.

use super::diagnostics::{Coverage, Diagnostic, Result};
use super::source::{PathEncoding, SourceSpan};
use super::symbols::Symbol;

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

#[derive(serde::Serialize, Clone, Debug)]
pub struct UseSite {
    pub file: String,
    pub span: SourceSpan,
    pub path_encoding: Option<PathEncoding>,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct Relationship {
    pub source: Symbol,
    pub target: Option<Symbol>,
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
