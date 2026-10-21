// Purpose: swift import bindings and aliases.
// Pattern: Simple Module.
// Usage: The import dispatcher delegates this language syntax.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{child_kind, span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn swift(&mut self, node: Node<'_>) -> Result<()> {
        let Some(path) = child_kind(node, &["identifier"]) else {
            return Ok(());
        };
        let raw = text(path, self.source);
        if let Some((module, name)) = raw.rsplit_once('.') {
            self.bind(name, module, Some(name), span(node))
        } else {
            self.bind("*", raw, Some("*"), span(node))
        }
    }
}
