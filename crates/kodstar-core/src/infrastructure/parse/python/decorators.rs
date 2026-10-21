// Purpose: python declaration decorator ownership and references.
// Pattern: Simple Module.
// Usage: Declaration extraction supplies the actual declaration context.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, span};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn python_decorators(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Python {
            return Ok(());
        }
        let Some(wrapper) = node.parent().filter(|p| p.kind() == "decorated_definition") else {
            return Ok(());
        };
        let mut cursor = wrapper.walk();
        for decorator in wrapper
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "decorator")
        {
            if let Some(value) = decorator.named_child(0) {
                let name = if value.kind() == "call" {
                    value.child_by_field_name("function").unwrap_or(value)
                } else {
                    value
                };
                self.usage(
                    context,
                    self.reference_text(name)?,
                    RelationKind::References,
                    span(name),
                )?;
            }
        }
        Ok(())
    }
}
