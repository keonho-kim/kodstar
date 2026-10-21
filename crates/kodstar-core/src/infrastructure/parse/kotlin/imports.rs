// Purpose: kotlin import bindings and aliases.
// Pattern: Simple Module.
// Usage: The import dispatcher delegates this language syntax.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{child_kind, span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn kotlin(&mut self, node: Node<'_>) -> Result<()> {
        let Some(path) = child_kind(node, &["qualified_identifier", "identifier"]) else {
            return Ok(());
        };
        let mut cursor = node.walk();
        let wildcard = node.children(&mut cursor).any(|n| n.kind() == "*");
        let mut cursor = node.walk();
        let alias = node
            .named_children(&mut cursor)
            .find(|n| n.id() != path.id() && n.kind() == "identifier");
        let mut cursor = path.walk();
        let mut parts = path
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "identifier")
            .peekable();
        let mut module = String::new();
        let mut name = if path.kind() == "identifier" {
            text(path, self.source)
        } else {
            "*"
        };
        while let Some(part) = parts.next() {
            let spelling = text(part, self.source);
            if parts.peek().is_some() || wildcard {
                if module
                    .len()
                    .saturating_add(spelling.len())
                    .saturating_add(1)
                    > self.limit
                {
                    return Err(crate::domain::diagnostics::Error::new(
                        crate::domain::diagnostics::ErrorKind::Limit,
                        "Kotlin import path exceeds fact_batch_bytes",
                    ));
                }
                if !module.is_empty() {
                    module.push('.');
                }
                module.push_str(spelling);
            } else {
                name = spelling;
            }
        }
        self.bind(
            alias.map(|n| text(n, self.source)).unwrap_or(name),
            &module,
            Some(name),
            span(node),
        )
    }
}
