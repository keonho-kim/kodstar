// Purpose: 엔진 공개 API와 빌드 버전의 선택적 노출.
// Pattern: Simple Module.
// Usage: 외부 엔진 소비자와 CLI가 import.
// Related: crates/kodstar-core/src/application/engine.rs, crates/kodstar-core/src/domain/results.rs.

//! KodStar analysis engine. Internal layers remain private behind the public engine API.

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("KodStar supports macOS and Linux only");

mod application;
mod domain;
mod infrastructure;

/// Engine build version, available without opening a project or initializing resources.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use application::analysis::InitOptions;
pub use application::cache::CacheRequest;
pub use application::engine::Engine;
pub use application::execution::Command;
pub use application::installation::{InitializationTarget, LocalDataLease, LocalDataRemoval};
pub use application::workspace::{ProjectOptions, Settings, SettingsOverrides};
pub use domain::cancellation::Cancellation;
pub use domain::diagnostics::{Coverage, Diagnostic, Error, ErrorKind, Result};
pub use domain::history::{Comparison, HistoryRequest, HistoryRow, HistoryState};
pub use domain::languages::Language;
pub use domain::query::{Depth, Limit, Query, QueryOptions, Refresh, Selector};
pub use domain::relations::{Direction, RELATION_REGISTRY_VERSION, RelationKind, Resolution};
pub use domain::results::{
    CacheEntry, FileConnection, InitReport, LanguageCapability, MaintenanceReport, Progress,
    QueryMetadata, QueryRow, QuerySink, QuerySummary, Relationship, Status, UseSite,
};
pub use domain::source::{PathEncoding, SourceSpan};
pub use domain::symbols::{DefinitionLocation, Symbol, SymbolKind};
