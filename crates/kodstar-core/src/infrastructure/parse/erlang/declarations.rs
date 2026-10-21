// Purpose: Erlang record declarations and arity-qualified functions.
// Pattern: Simple Module.
// Usage: Language dispatch invokes these syntax rules.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Extractor, text};
use tree_sitter::Node;
impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn erlang_record_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language == Language::Erlang
            && node.kind() == "record_decl"
            && let Some(name) = node.child_by_field_name("name")
        {
            return Ok(Some((
                self.bounded_text(text(name, self.source).trim_matches('\''))?,
                SymbolKind::Struct,
            )));
        }
        Ok(None)
    }
    pub(in crate::infrastructure::parse) fn erlang_function_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language == Language::Erlang
            && node.kind() == "fun_decl"
            && let Some(clause) = node.child_by_field_name("clause")
            && let Some(name) = clause.child_by_field_name("name")
        {
            let arity = clause
                .child_by_field_name("args")
                .map_or(0, |args| args.named_child_count());
            return Ok(Some((
                format!(
                    "{}/{arity}",
                    self.bounded_text(text(name, self.source).trim_matches('\''))?
                ),
                SymbolKind::Function,
            )));
        }
        Ok(None)
    }
}
