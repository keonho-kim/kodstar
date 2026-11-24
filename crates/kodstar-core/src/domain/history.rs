// Purpose: 로컬 이력 요청과 스트리밍 결과 계약.
// Pattern: Simple Module.
// Usage: Engine과 CLI·세션이 체크포인트와 브랜치를 교환.
// Related: crates/kodstar-core/src/application/history/mod.rs.

use super::diagnostics::{Error, Result};
use super::query::Refresh;
use super::source::PathEncoding;
use std::path::PathBuf;

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryRequest {
    Upgrade,
    List {
        #[serde(default)]
        refresh: Refresh,
    },
    Save {
        name: String,
    },
    Diff {
        from: Option<String>,
        to: Option<String>,
        #[serde(default)]
        refresh: Refresh,
    },
    /// Compare relationship evidence at two checkpoints using the current analyzer.
    GraphDiff {
        from: Option<String>,
        to: Option<String>,
        #[serde(default)]
        refresh: Refresh,
        /// Zero removes the output row limit.
        limit: Option<usize>,
    },
    Restore {
        revision: String,
        path: Option<PathBuf>,
    },
    Branches,
    CreateBranch {
        name: String,
    },
    SwitchBranch {
        name: String,
    },
    DeleteBranch {
        name: String,
    },
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct HistoryState {
    pub comparison: Option<Comparison>,
    pub repository: String,
    pub branch: String,
    pub checkpoint: String,
    pub created: bool,
    pub changed_files: u64,
    pub tracked_files: u64,
    pub analysis_checkpoint: Option<String>,
    pub analysis_error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Comparison {
    pub from: Option<String>,
    pub to: String,
    pub before_coverage: Option<super::diagnostics::Coverage>,
    pub after_coverage: Option<super::diagnostics::Coverage>,
    pub configuration: Option<String>,
    pub extraction_version: Option<u32>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryRow {
    GraphChange {
        change: String,
        before: Option<Box<GraphEvidence>>,
        after: Option<Box<GraphEvidence>>,
    },
    Impact {
        checkpoint: String,
        relationship: Box<super::results::Relationship>,
    },
    Checkpoint {
        id: String,
        parent: Option<String>,
        created_at: u64,
        name: Option<String>,
        changed_files: u64,
    },
    Branch {
        name: String,
        checkpoint: String,
        current: bool,
    },
    Change {
        path: String,
        path_encoding: Option<PathEncoding>,
        change: String,
        before: Option<String>,
        after: Option<String>,
        before_kind: Option<String>,
        after_kind: Option<String>,
        before_mode: Option<u32>,
        after_mode: Option<u32>,
    },
    DiffLine {
        path: String,
        path_encoding: Option<PathEncoding>,
        side: String,
        line: u64,
        text: String,
    },
    DiffOmitted {
        path: String,
        path_encoding: Option<PathEncoding>,
        side: String,
        reason: String,
    },
    Operation {
        operation: String,
        checkpoint: String,
        branch: String,
        changed_files: u64,
    },
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct GraphEvidence {
    pub source: String,
    pub target: Option<GraphTarget>,
    pub external: Option<String>,
    pub relation: String,
    pub expression: String,
    pub resolution: String,
    pub rule: String,
    pub condition: Option<String>,
    pub type_only: bool,
    pub count: u64,
    pub site: super::results::UseSite,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct GraphTarget {
    pub name: String,
    pub file: String,
    pub path_encoding: Option<PathEncoding>,
}

pub(crate) fn validate_name(value: &str) -> Result<()> {
    let reserved = value
        .strip_prefix('c')
        .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()));
    if value.len() > 128
        || value.is_empty()
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || reserved
    {
        return Err(Error::argument(
            "name must start with a letter or digit, use at most 128 ASCII letters/digits/._-, and not be a checkpoint ID",
        ));
    }
    Ok(())
}
