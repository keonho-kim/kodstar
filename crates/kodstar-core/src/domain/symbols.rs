// Purpose: 객체 종류·논리 정체성과 정의 위치.
// Pattern: Simple Module.
// Usage: 저장 행과 공개 조회 결과가 사용.
// Related: crates/kodstar-core/src/infrastructure/index/mod.rs.

//! Symbol identity and multiple source definitions are distinct from display names.

use super::source::{PathEncoding, SourceSpan};

#[derive(serde::Serialize, Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SymbolKind {
    File,
    Module,
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Trait,
    Enum,
    Type,
    Field,
    Constructor,
    Macro,
}

impl SymbolKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Module => "module",
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Enum => "enum",
            Self::Type => "type",
            Self::Field => "field",
            Self::Constructor => "constructor",
            Self::Macro => "macro",
        }
    }
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct Symbol {
    pub id: i64,
    pub logical_key: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub language: String,
    pub module: String,
    pub package: String,
    pub scope: u32,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_encoding: Option<PathEncoding>,
    pub definition: SourceSpan,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub definitions: Vec<DefinitionLocation>,
    pub signature: String,
    pub condition: Option<String>,
    pub owner: Option<i64>,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct DefinitionLocation {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_encoding: Option<PathEncoding>,
    pub span: SourceSpan,
}
