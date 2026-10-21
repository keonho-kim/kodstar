// Purpose: 관계 종류·방향·해석 상태의 공개 계약.
// Pattern: Simple Module.
// Usage: 구문 사실과 조회 옵션에서 사용.
// Related: crates/kodstar-core/src/domain/facts.rs, crates/kodstar-core/src/domain/query.rs.

//! Versioned relationship names and traversal policy.

use std::fmt;
use std::str::FromStr;

use super::diagnostics::{Error, Result};

pub const RELATION_REGISTRY_VERSION: u32 = 2;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Calls,
    IndirectCall,
    References,
    UsesType,
    Imports,
    Reexports,
    Includes,
    Inherits,
    MixesIn,
    Implements,
    Overrides,
    DispatchCandidate,
    Defines,
    Contains,
    TemplateInclude,
    TemplateExtends,
    DependsOn,
    EventHandler,
}

impl RelationKind {
    pub const ALL: [Self; 18] = [
        Self::Calls,
        Self::IndirectCall,
        Self::References,
        Self::UsesType,
        Self::Imports,
        Self::Reexports,
        Self::Includes,
        Self::Inherits,
        Self::MixesIn,
        Self::Implements,
        Self::Overrides,
        Self::DispatchCandidate,
        Self::Defines,
        Self::Contains,
        Self::TemplateInclude,
        Self::TemplateExtends,
        Self::DependsOn,
        Self::EventHandler,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Calls => "calls",
            Self::IndirectCall => "indirect_call",
            Self::References => "references",
            Self::UsesType => "uses_type",
            Self::Imports => "imports",
            Self::Reexports => "reexports",
            Self::Includes => "includes",
            Self::Inherits => "inherits",
            Self::MixesIn => "mixes_in",
            Self::Implements => "implements",
            Self::Overrides => "overrides",
            Self::DispatchCandidate => "dispatch_candidate",
            Self::Defines => "defines",
            Self::Contains => "contains",
            Self::TemplateInclude => "template_include",
            Self::TemplateExtends => "template_extends",
            Self::DependsOn => "depends_on",
            Self::EventHandler => "event_handler",
        }
    }

    pub const fn is_dependency(self) -> bool {
        !matches!(self, Self::Defines | Self::Contains)
    }
}

impl fmt::Display for RelationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RelationKind {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|relation| relation.as_str() == value)
            .ok_or_else(|| Error::argument(format!("unknown relationship kind: {value}")))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Direction {
    In,
    Out,
    #[default]
    Both,
}

impl FromStr for Direction {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "in" => Ok(Self::In),
            "out" => Ok(Self::Out),
            "both" => Ok(Self::Both),
            _ => Err(Error::argument(format!("unknown direction: {value}"))),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Resolved,
    Candidate,
    Ambiguous,
    Unresolved,
}

impl Resolution {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Candidate => "candidate",
            Self::Ambiguous => "ambiguous",
            Self::Unresolved => "unresolved",
        }
    }
}
