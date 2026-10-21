// Purpose: Python property·명시적 반환 타입과 getter 읽기 사실 추출.
// Pattern: Simple Module.
// Usage: 함수 선언·참조 순회가 Python 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/application/analysis/resolve/python/properties.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn python_callable(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Python {
            return Ok(());
        }
        self.python_fixture(node, context)?;
        if let Some(ty) = node.child_by_field_name("return_type") {
            let value = text(ty, self.source);
            let value = value
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .or_else(|| value.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
                .unwrap_or(value);
            let annotation = if value
                .chars()
                .all(|c| c.is_alphanumeric() || "_.".contains(c))
            {
                self.bounded_text(value)?
            } else {
                self.type_text(ty)?
            };
            self.return_annotation(context, ty, annotation)?;
        }
        let Some(wrapper) = node.parent().filter(|n| n.kind() == "decorated_definition") else {
            return Ok(());
        };
        let mut cursor = wrapper.walk();
        for decorator in wrapper
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "decorator")
        {
            let Some(value) = decorator.named_child(0) else {
                continue;
            };
            let name = text(value, self.source);
            if matches!(
                name,
                "property" | "cached_property" | "functools.cached_property"
            ) || name.ends_with(".getter")
            {
                let name = self.bounded_text(name)?;
                (self.emit)(Fact::Binding(Binding {
                    scope: context.scope,
                    name: "@property".into(),
                    target: Some(context.source),
                    module: None,
                    imported_name: Some(name),
                    type_name: None,
                    kind: "python_property",
                    span: span(decorator),
                }))?;
            }
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn python_property_read(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Python || node.kind() != "attribute" {
            return Ok(());
        }
        let mut current = node;
        while let Some(parent) = current.parent() {
            if matches!(
                parent.kind(),
                "pattern_list" | "tuple_pattern" | "list_pattern"
            ) {
                current = parent;
                continue;
            }
            if (matches!(
                parent.kind(),
                "assignment" | "for_statement" | "for_in_clause"
            ) && parent
                .child_by_field_name("left")
                .is_some_and(|n| n.id() == current.id()))
                || parent.kind() == "delete_statement"
                || parent.kind() == "call"
                    && parent
                        .child_by_field_name("function")
                        .is_some_and(|n| n.id() == current.id())
            {
                return Ok(());
            }
            break;
        }
        let expression = self.reference_text(node)?;
        self.usage(
            context,
            format!("@property:{expression}"),
            RelationKind::References,
            span(node),
        )
    }
}
