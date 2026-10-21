// Purpose: C/C++ qualified definitions and callable declarations.
// Pattern: Simple Module.
// Usage: Language dispatch invokes these syntax rules.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::syntax::name_node;
use crate::infrastructure::parse::{Extractor, text};
use tree_sitter::Node;
impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn cxx_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language == Language::Cpp
            && node.kind() == "function_definition"
            && let Some(name) = crate::infrastructure::parse::cxx::declarators::qualified_name(node)
        {
            return Ok(Some((
                self.bounded_text(text(
                    crate::infrastructure::parse::cxx::declarators::leaf(name),
                    self.source,
                ))?,
                SymbolKind::Method,
            )));
        }
        if matches!(self.language, Language::C | Language::Cpp)
            && matches!(node.kind(), "declaration" | "field_declaration")
            && let Some(declarator) = node
                .child_by_field_name("declarator")
                .filter(|d| d.kind() == "function_declarator")
            && let Some(name) = name_node(declarator)
        {
            return Ok(Some((
                self.bounded_text(text(name, self.source))?,
                if node.kind() == "field_declaration" {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                },
            )));
        }
        Ok(None)
    }
}
