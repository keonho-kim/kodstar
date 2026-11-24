// Purpose: Shared explicit nominal declaration shapes; no name or type inference.
// Pattern: Simple Module.
// Usage: Language declaration dispatch selects the applicable nominal grammar.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Extractor, child_kind, syntax::name_node, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn nominal_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if matches!(
            self.language,
            Language::Go | Language::Swift | Language::Kotlin
        ) && matches!(node.kind(), "type_spec" | "class_declaration")
            && let Some(name) = name_node(node)
        {
            let mut cursor = node.walk();
            let token = node
                .children(&mut cursor)
                .find(|n| matches!(n.kind(), "enum" | "struct" | "interface"));
            let ty = node.child_by_field_name("type");
            let kind = if token.is_some_and(|n| n.kind() == "enum")
                || child_kind(node, &["modifiers"])
                    .is_some_and(|n| text(n, self.source).split_whitespace().any(|s| s == "enum"))
            {
                SymbolKind::Enum
            } else if token.is_some_and(|n| n.kind() == "interface")
                || ty.is_some_and(|n| n.kind() == "interface_type")
            {
                SymbolKind::Interface
            } else if token.is_some_and(|n| n.kind() == "struct")
                || ty.is_some_and(|n| n.kind() == "struct_type")
            {
                SymbolKind::Struct
            } else if self.language == Language::Go {
                SymbolKind::Type
            } else {
                SymbolKind::Class
            };
            return Ok(Some((self.bounded_text(text(name, self.source))?, kind)));
        }
        Ok(None)
    }
}
