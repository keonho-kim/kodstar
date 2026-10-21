// Purpose: cxx import syntax and binding facts.
// Pattern: Simple Module.
// Usage: The common import dispatcher selects this language.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::{Imports, unquote};
use crate::infrastructure::parse::{span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn cxx(&mut self, node: Node<'_>) -> Result<()> {
        if let Some(path) = node.child_by_field_name("path") {
            self.binding_kind = Some("include");
            self.bind(
                "*",
                &format!("@include:{}", unquote(text(path, self.source))),
                None,
                span(path),
            )?;
        }
        Ok(())
    }
}
