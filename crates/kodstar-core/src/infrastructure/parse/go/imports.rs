// Purpose: Language import syntax and explicit binding extraction.
// Pattern: Simple Module.
// Usage: The import dispatcher passes one language declaration.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::{Imports, unquote};
use crate::infrastructure::parse::{span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn go(&mut self, node: Node<'_>) -> Result<()> {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "import_spec_list" {
                self.go(child)?;
                continue;
            }
            if child.kind() != "import_spec" {
                continue;
            }
            if let Some(path) = child.child_by_field_name("path") {
                let module = unquote(text(path, self.source));
                self.binding_kind = child
                    .child_by_field_name("name")
                    .is_none()
                    .then_some("go_import");
                let name = child
                    .child_by_field_name("name")
                    .map(|node| text(node, self.source))
                    .unwrap_or_else(|| module.rsplit('/').next().unwrap_or(module));
                self.bind(
                    if name == "." { "*" } else { name },
                    module,
                    None,
                    span(child),
                )?;
            }
        }
        Ok(())
    }
}
