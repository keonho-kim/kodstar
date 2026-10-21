// Purpose: erlang import syntax and binding facts.
// Pattern: Simple Module.
// Usage: The common import dispatcher selects this language.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn erlang(&mut self, node: Node<'_>) -> Result<()> {
        if let Some(module) = node.child_by_field_name("module") {
            let module_name = text(module, self.source).trim_matches('\'');
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "fa" {
                    let Some(name) =
                        crate::infrastructure::parse::erlang::function_arity(child, self.source)
                    else {
                        continue;
                    };
                    self.bind(&name, module_name, Some(&name), span(child))?;
                }
            }
        }
        Ok(())
    }
}
