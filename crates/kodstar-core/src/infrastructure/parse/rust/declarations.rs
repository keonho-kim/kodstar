// Purpose: Rust const and static declarations.
// Pattern: Simple Module.
// Usage: Language dispatch invokes these syntax rules.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Extractor, text};
use tree_sitter::Node;
impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn rust_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language == Language::Rust
            && matches!(node.kind(), "const_item" | "static_item")
            && let Some(name) = node.child_by_field_name("name")
            && text(name, self.source) != "_"
        {
            return Ok(Some((
                self.bounded_text(text(name, self.source))?,
                SymbolKind::Field,
            )));
        }
        Ok(None)
    }
}
