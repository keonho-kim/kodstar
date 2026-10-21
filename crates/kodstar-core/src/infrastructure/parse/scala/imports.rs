// Purpose: scala import bindings and aliases.
// Pattern: Simple Module.
// Usage: The import dispatcher delegates this language syntax.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{child_kind, span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn scala(&mut self, node: Node<'_>) -> Result<()> {
        let Some(selectors) = child_kind(node, &["namespace_selectors"]) else {
            return self.qualified(node, '.');
        };
        let mut module = String::new();
        let mut cursor = node.walk();
        for path in node.children_by_field_name("path", &mut cursor) {
            let value = text(path, self.source);
            if module.len().saturating_add(value.len()).saturating_add(1) > self.limit {
                return Err(crate::domain::diagnostics::Error::new(
                    crate::domain::diagnostics::ErrorKind::Limit,
                    "import path exceeds fact_batch_bytes",
                ));
            }
            if !module.is_empty() {
                module.push('.');
            }
            module.push_str(value);
        }
        let mut cursor = selectors.walk();
        for selector in selectors.named_children(&mut cursor) {
            let name = selector.child_by_field_name("name").unwrap_or(selector);
            let alias = selector.child_by_field_name("alias").unwrap_or(name);
            let name = text(name, self.source);
            let alias = text(alias, self.source);
            if alias == "_" && name != "_" {
                continue;
            }
            self.bind(
                if matches!(alias, "_" | "*") {
                    "*"
                } else {
                    alias
                },
                &module,
                Some(if name == "_" { "*" } else { name }),
                span(selector),
            )?;
        }
        Ok(())
    }
}
