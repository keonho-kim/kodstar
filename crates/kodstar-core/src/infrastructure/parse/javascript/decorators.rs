// Purpose: javascript declaration decorator ownership and references.
// Pattern: Simple Module.
// Usage: Declaration extraction supplies the actual declaration context.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, span};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn typescript_decorators(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::TypeScript {
            return Ok(());
        }
        self.direct_decorators(node, context)?;
        if matches!(
            node.kind(),
            "class_declaration" | "abstract_class_declaration"
        ) {
            if let Some(export) = node.parent().filter(|n| n.kind() == "export_statement") {
                self.direct_decorators(export, context)?;
            }
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for member in body.named_children(&mut cursor) {
                    if member.kind() == "decorator" {
                        let mut following = member.next_named_sibling();
                        while following.is_some_and(|n| matches!(n.kind(), "decorator" | "comment"))
                        {
                            following = following.and_then(|n| n.next_named_sibling());
                        }
                        if following.is_none_or(|n| n.kind() != "method_definition") {
                            self.decorator_reference(member, context)?;
                        }
                    } else if member.kind() != "method_definition" {
                        self.direct_decorators(member, context)?;
                    }
                }
            }
        } else if node.kind() == "method_definition" {
            let mut previous = node.prev_named_sibling();
            while let Some(sibling) =
                previous.filter(|n| matches!(n.kind(), "decorator" | "comment"))
            {
                if sibling.kind() == "decorator" {
                    self.decorator_reference(sibling, context)?;
                }
                previous = sibling.prev_named_sibling();
            }
            if let Some(parameters) = node.child_by_field_name("parameters") {
                let mut cursor = parameters.walk();
                for parameter in parameters.named_children(&mut cursor) {
                    self.direct_decorators(parameter, context)?;
                }
            }
        }
        Ok(())
    }

    fn direct_decorators(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        let mut cursor = node.walk();
        for decorator in node
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "decorator")
        {
            self.decorator_reference(decorator, context)?;
        }
        Ok(())
    }

    fn decorator_reference(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        let Some(value) = node.named_child(0) else {
            return Ok(());
        };
        let value = if value.kind() == "call_expression" {
            value.child_by_field_name("function").unwrap_or(value)
        } else {
            value
        };
        self.usage(
            context,
            self.reference_text(value)?,
            RelationKind::References,
            span(value),
        )
    }
}
