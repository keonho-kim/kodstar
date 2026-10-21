// Purpose: 이름·getattr·명시적 함수 값과 accessor 구문 사실 추출.
// Pattern: Simple Module.
// Usage: 공통 참조 추출기가 AST 노드와 현재 소유 scope를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/references.rs, crates/kodstar-core/src/application/analysis/resolve/callable_values.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn callable_values(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        if !matches!(
            self.language,
            Language::Python | Language::JavaScript | Language::TypeScript
        ) {
            return Ok(());
        }
        self.callable_accessor(node, context)?;
        self.getattr_value(node, context)?;
        if matches!(node.kind(), "call" | "call_expression" | "new_expression")
            && let Some(arguments) = node.child_by_field_name("arguments")
        {
            let mut cursor = arguments.walk();
            for argument in arguments.named_children(&mut cursor) {
                let value = if argument.kind() == "keyword_argument" {
                    argument.child_by_field_name("value")
                } else {
                    Some(argument)
                };
                if let Some(value) = value {
                    self.callable_value(value, context, "argument")?;
                }
            }
        }
        if matches!(
            node.kind(),
            "dictionary" | "object" | "list" | "set" | "tuple" | "array"
        ) {
            let mut cursor = node.walk();
            for element in node.named_children(&mut cursor) {
                let value = if element.kind() == "pair" {
                    element.child_by_field_name("value")
                } else {
                    Some(element)
                };
                if let Some(value) = value {
                    self.callable_value(value, context, "collection")?;
                }
            }
        }
        if matches!(
            node.kind(),
            "assignment" | "assignment_expression" | "variable_declarator"
        ) && let Some(value) = node
            .child_by_field_name("right")
            .or_else(|| node.child_by_field_name("value"))
        {
            self.callable_value(value, context, "assignment")?;
        }
        Ok(())
    }

    fn callable_value(
        &mut self,
        value: Node<'_>,
        context: &Context,
        role: &'static str,
    ) -> Result<()> {
        if !matches!(value.kind(), "identifier" | "shorthand_property_identifier") {
            return Ok(());
        }
        let expression = self.bounded_text(text(value, self.source))?;
        self.named_callable_value(value, context, role, expression)
    }

    pub(super) fn named_callable_value(
        &mut self,
        value: Node<'_>,
        context: &Context,
        role: &'static str,
        expression: String,
    ) -> Result<()> {
        (self.emit)(Fact::Binding(Binding {
            scope: context.scope,
            name: "@callable-value".into(),
            target: None,
            module: Some(role.into()),
            imported_name: None,
            type_name: None,
            kind: "callable_value",
            span: span(value),
        }))?;
        self.usage(
            context,
            expression.clone(),
            RelationKind::References,
            span(value),
        )?;
        self.usage(context, expression, RelationKind::IndirectCall, span(value))
    }
    fn getattr_value(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        if self.language != Language::Python
            || node.kind() != "call"
            || !node
                .child_by_field_name("function")
                .is_some_and(|n| n.kind() == "identifier" && text(n, self.source) == "getattr")
        {
            return Ok(());
        }
        let Some(arguments) = node.child_by_field_name("arguments") else {
            return Ok(());
        };
        let (Some(receiver), Some(attribute)) =
            (arguments.named_child(0), arguments.named_child(1))
        else {
            return Ok(());
        };
        let Some(name) =
            crate::infrastructure::parse::python::literals::plain_string(attribute, self.source)
        else {
            return Ok(());
        };
        let mut expression = String::new();
        if receiver.kind() == "call" {
            let producer = self.call_text(receiver, self.callee(receiver).unwrap_or(receiver))?;
            self.append_signature(&mut expression, &["@result:", &producer, "#", name])?;
        } else {
            let receiver = self.reference_text(receiver)?;
            self.append_signature(&mut expression, &[&receiver, ".", name])?;
        }
        self.named_callable_value(attribute, context, "getattr", expression)
    }

    fn callable_accessor(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        let accessor = if self.language == Language::Python && node.kind() == "function_definition"
        {
            node.parent()
                .filter(|n| n.kind() == "decorated_definition")
                .is_some_and(|wrapper| {
                    let mut cursor = wrapper.walk();
                    wrapper
                        .named_children(&mut cursor)
                        .filter(|n| n.kind() == "decorator")
                        .any(|n| {
                            n.named_child(0).is_some_and(|n| {
                                text(n, self.source).ends_with(".setter")
                                    || text(n, self.source).ends_with(".deleter")
                            })
                        })
                })
        } else if matches!(
            node.kind(),
            "method_definition" | "method_signature" | "abstract_method_signature"
        ) {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .any(|n| matches!(n.kind(), "get" | "set"))
        } else {
            false
        };
        if accessor {
            (self.emit)(Fact::Binding(Binding {
                scope: context.scope,
                name: "@callable-accessor".into(),
                target: Some(context.source),
                module: None,
                imported_name: None,
                type_name: None,
                kind: "callable_accessor",
                span: span(node),
            }))?;
        }
        Ok(())
    }
}
