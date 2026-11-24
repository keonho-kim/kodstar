// Purpose: 파일별 구문 사실과 소유 바이트 계산.
// Pattern: Simple Module.
// Usage: parser가 생성하고 index가 저장.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/infrastructure/index/write.rs.

//! File-local syntax events. The consumer persists each event rather than retaining file results.

pub(crate) mod append;
mod fingerprint;
pub(crate) use fingerprint::{Fingerprinter, Fingerprints};

pub(crate) const CONDITIONAL_EXPORT: &str = "<conditional-export>";

use super::relations::RelationKind;
use super::source::SourceSpan;
use super::symbols::SymbolKind;

#[derive(serde::Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct Definition {
    pub local_id: u32,
    pub owner: Option<u32>,
    pub scope: u32,
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub condition: Option<String>,
    pub span: SourceSpan,
    pub exported: bool,
}

impl Definition {
    pub(crate) fn payload_bytes(&self) -> usize {
        self.name.capacity()
            + self.qualified_name.capacity()
            + self.signature.capacity()
            + self.condition.as_ref().map_or(0, String::capacity)
    }
}

#[derive(serde::Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct Scope {
    pub local_id: u32,
    pub parent: Option<u32>,
    pub span: SourceSpan,
}

#[derive(serde::Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct Binding {
    pub scope: u32,
    pub name: String,
    pub target: Option<u32>,
    pub module: Option<String>,
    pub imported_name: Option<String>,
    pub type_name: Option<String>,
    pub kind: &'static str,
    pub span: SourceSpan,
}

#[derive(serde::Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct SyntaxUse {
    pub source: u32,
    pub scope: u32,
    pub expression: String,
    pub relation: RelationKind,
    pub span: SourceSpan,
    pub condition: Option<String>,
    pub type_only: bool,
}

#[derive(serde::Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Fact {
    Definition(Definition),
    Scope(Scope),
    Binding(Binding),
    Use(SyntaxUse),
    Module {
        name: String,
        declaration_only: bool,
    },
}

impl Fact {
    pub(crate) fn owned_bytes(&self) -> usize {
        let extra = match self {
            Self::Definition(value) => value.payload_bytes(),
            Self::Scope(_) => 0,
            Self::Binding(value) => {
                value.name.capacity()
                    + [&value.module, &value.imported_name, &value.type_name]
                        .into_iter()
                        .map(|value| value.as_ref().map_or(0, String::capacity))
                        .sum::<usize>()
            }
            Self::Use(value) => {
                value.expression.capacity() + value.condition.as_ref().map_or(0, String::capacity)
            }
            Self::Module { name, .. } => name.capacity(),
        };
        std::mem::size_of::<Self>() + extra
    }
}
