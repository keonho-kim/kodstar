// Purpose: 객체 selector와 조회 옵션·한도 계약.
// Pattern: Simple Module.
// Usage: Engine 조회와 CLI 소비자가 사용.
// Related: crates/kodstar-core/src/application/engine.rs.

//! Validated query requests shared by CLI and engine consumers.

mod validation;

use std::path::PathBuf;

use super::diagnostics::{Error, Result};
use super::relations::{Direction, RelationKind};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Refresh {
    #[default]
    Metadata,
    Verify,
    None,
}

impl Refresh {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Metadata => "metadata_checked",
            Self::Verify => "content_verified",
            Self::None => "not_checked",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Selector {
    pub file: PathBuf,
    pub name: String,
    pub at: Option<(usize, usize)>,
}

impl Selector {
    /// Locate the separator after a file extension, preserving drive letters and `::` in names.
    pub fn parse(value: &str) -> Result<Self> {
        let separator = value
            .match_indices("::")
            .find(|(index, _)| {
                let file = &value[..*index];
                PathBuf::from(file).extension().is_some()
                    && !file.ends_with('/')
                    && !file.ends_with('\\')
            })
            .map(|(index, _)| index)
            .or_else(|| value.find("::"));
        let index = separator.ok_or_else(|| Error::argument("expected <file::qualified-name>"))?;
        let name = &value[index + 2..];
        if name.is_empty() || index == 0 {
            return Err(Error::argument("object name cannot be empty"));
        }
        Ok(Self {
            file: PathBuf::from(&value[..index]),
            name: name.into(),
            at: None,
        })
    }
}

#[derive(Clone, Debug)]
pub enum Query {
    Symbols {
        file: PathBuf,
    },
    Files {
        file: PathBuf,
    },
    Related {
        selector: Selector,
    },
    Deps {
        selector: Selector,
    },
    Refs {
        selector: Selector,
    },
    Explain {
        selector: Selector,
    },
    Impact {
        file: PathBuf,
        name: Option<String>,
        at: Option<(usize, usize)>,
    },
    Path {
        from: Selector,
        to: Selector,
    },
    Search {
        name: String,
        prefix: bool,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub enum Limit {
    #[default]
    Default,
    Count(std::num::NonZeroUsize),
    Unlimited,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum Depth {
    #[default]
    Default,
    Bounded(std::num::NonZeroUsize),
    Transitive,
}

impl Limit {
    pub(crate) fn resolve(self, default: usize) -> Option<usize> {
        match self {
            Self::Default => Some(default),
            Self::Count(value) => Some(value.get()),
            Self::Unlimited => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct QueryOptions {
    pub refresh: Refresh,
    pub direction: Option<Direction>,
    pub relations: Vec<RelationKind>,
    pub depth: Depth,
    pub members: bool,
    pub sites: bool,
    pub include_candidates: bool,
    pub include_external: bool,
    pub limit: Limit,
    pub max_nodes: Limit,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            refresh: Refresh::Metadata,
            direction: None,
            relations: Vec::new(),
            depth: Depth::Default,
            members: false,
            sites: false,
            include_candidates: false,
            include_external: false,
            limit: Limit::Default,
            max_nodes: Limit::Default,
        }
    }
}

impl QueryOptions {
    pub(crate) fn validate(&self, query: &Query) -> Result<()> {
        validation::validate(self, query)
    }

    pub(crate) fn depth_for(&self, query: &Query) -> Option<usize> {
        match self.depth {
            Depth::Default => {
                if matches!(query, Query::Impact { .. } | Query::Path { .. }) {
                    None
                } else {
                    Some(1)
                }
            }
            Depth::Bounded(depth) => Some(depth.get()),
            Depth::Transitive => None,
        }
    }

    pub(crate) fn direction_for(&self, query: &Query) -> Direction {
        self.direction.unwrap_or(match query {
            Query::Deps { .. } => Direction::Out,
            Query::Refs { .. } | Query::Impact { .. } => Direction::In,
            _ => Direction::Both,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_selector_keeps_windows_drive_and_qualified_symbol() -> Result<()> {
        let selector = Selector::parse(r"C:\work\api.rs::Service::run")?;
        assert_eq!(selector.file, PathBuf::from(r"C:\work\api.rs"));
        assert_eq!(selector.name, "Service::run");
        assert!(Selector::parse("src/api.rs::").is_err());
        Ok(())
    }
}
