// Purpose: Zig named container·tagged union·error set의 정의와 case 구문 분류.
// Pattern: Simple Module.
// Usage: 특수 선언 분류와 enum case 추출기가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs, crates/kodstar-core/src/infrastructure/parse/members.rs.

pub(super) mod imports;

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Extractor, child_kind, text};
use tree_sitter::Node;

pub(in crate::infrastructure::parse) fn case(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if node.kind() == "identifier" && parent.kind() == "error_set_declaration" {
        return true;
    }
    if node.kind() != "container_field" {
        return false;
    }
    if parent.kind() == "enum_declaration" {
        return true;
    }
    if parent.kind() != "union_declaration" {
        return false;
    }
    let mut cursor = parent.walk();
    parent.children(&mut cursor).any(|n| n.kind() == "(")
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn zig_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language != Language::Zig {
            return Ok(None);
        }
        let kind = match node.kind() {
            "struct_declaration" => SymbolKind::Struct,
            "enum_declaration" | "error_set_declaration" => SymbolKind::Enum,
            "union_declaration" => SymbolKind::Type,
            _ => return Ok(None),
        };
        let name = node
            .parent()
            .filter(|p| p.kind() == "variable_declaration")
            .and_then(|p| child_kind(p, &["identifier"]));
        name.map(|n| Ok((self.bounded_text(text(n, self.source))?, kind)))
            .transpose()
    }
}
