// Purpose: Language import syntax and explicit binding extraction.
// Pattern: Simple Module.
// Usage: The import dispatcher passes one language declaration.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn python(&mut self, node: Node<'_>) -> Result<()> {
        let module = node.child_by_field_name("module_name");
        let module_text = module.map(|module| text(module, self.source));
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if module.is_some_and(|module| module.id() == child.id()) {
                continue;
            }
            if child.kind() == "wildcard_import" {
                self.bind("*", module_text.unwrap_or(""), Some("*"), span(child))?;
                continue;
            }
            if !matches!(child.kind(), "dotted_name" | "aliased_import") {
                continue;
            }
            let name = child.child_by_field_name("name").unwrap_or(child);
            let raw = text(name, self.source);
            let alias = child
                .child_by_field_name("alias")
                .map(|node| text(node, self.source));
            if let Some(module) = module_text {
                self.bind(alias.unwrap_or(raw), module, Some(raw), span(child))?;
            } else if let Some(alias) = alias {
                self.bind(alias, raw, None, span(child))?;
            } else {
                let first = raw.split('.').next().unwrap_or(raw);
                self.bind(first, first, None, span(child))?;
            }
        }
        Ok(())
    }
}
