// Purpose: pytest fixture 이름·decorator·반환/yield 근거 추출.
// Pattern: Simple Module.
// Usage: Python 함수 메타데이터 추출기가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/python/members.rs, crates/kodstar-core/src/application/analysis/resolve/python/fixtures.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn python_fixture(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        let Some(wrapper) = node.parent().filter(|p| p.kind() == "decorated_definition") else {
            return Ok(());
        };
        let mut cursor = wrapper.walk();
        for decorator in wrapper
            .named_children(&mut cursor)
            .filter(|p| p.kind() == "decorator")
        {
            let Some(value) = decorator.named_child(0) else {
                continue;
            };
            let name = value.child_by_field_name("function").unwrap_or(value);
            let name = text(name, self.source);
            if name.rsplit('.').next() != Some("fixture") {
                continue;
            }
            let Some(original) = node.child_by_field_name("name") else {
                continue;
            };
            let mut fixture_name = self.bounded_text(text(original, self.source))?;
            if let Some(args) = value.child_by_field_name("arguments") {
                let mut values = args.walk();
                for arg in args.named_children(&mut values) {
                    if arg
                        .child_by_field_name("name")
                        .is_some_and(|n| text(n, self.source) == "name")
                    {
                        let Some(v) = arg
                            .child_by_field_name("value")
                            .filter(|n| n.kind() == "string")
                        else {
                            return Ok(());
                        };
                        let raw = text(v, self.source).trim_matches(['\'', '"']);
                        if raw.is_empty() || !raw.chars().all(|c| c.is_alphanumeric() || c == '_') {
                            return Ok(());
                        }
                        fixture_name = self.bounded_text(raw)?;
                    }
                }
            }
            let decorator = self.bounded_text(name)?;
            (self.emit)(Fact::Binding(Binding {
                scope: context.scope,
                name: fixture_name,
                target: Some(context.source),
                module: None,
                imported_name: Some(decorator),
                type_name: None,
                kind: "python_fixture",
                span: span(original),
            }))?;
            if let Some(ty) = node.child_by_field_name("return_type") {
                let annotation = self.type_text(ty)?;
                let annotation = fixture_annotation(&annotation);
                self.fixture_evidence(
                    node,
                    context,
                    "python_fixture_type",
                    Some(annotation.to_owned()),
                    None,
                )?;
            } else if let Some(body) = node.child_by_field_name("body") {
                self.fixture_returns(body, context)?;
            }
            break;
        }
        Ok(())
    }

    fn fixture_returns(&mut self, body: Node<'_>, context: &Context) -> Result<()> {
        let mut cursor = body.walk();
        loop {
            let node = cursor.node();
            if matches!(node.kind(), "return_statement" | "yield") {
                let producer = node
                    .named_child(0)
                    .filter(|n| n.kind() == "call")
                    .and_then(|n| n.child_by_field_name("function"));
                let expression = producer.map(|n| self.reference_text(n)).transpose()?;
                self.fixture_evidence(
                    node,
                    context,
                    if expression.is_some() {
                        "python_fixture_factory"
                    } else {
                        "python_fixture_unknown"
                    },
                    None,
                    expression,
                )?;
            } else if !matches!(
                node.kind(),
                "function_definition" | "class_definition" | "lambda"
            ) && cursor.goto_first_child()
            {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return Ok(());
                }
            }
        }
    }

    fn fixture_evidence(
        &mut self,
        node: Node<'_>,
        context: &Context,
        kind: &'static str,
        type_name: Option<String>,
        imported_name: Option<String>,
    ) -> Result<()> {
        (self.emit)(Fact::Binding(Binding {
            scope: context.scope,
            name: "@fixture-return".into(),
            target: Some(context.source),
            module: None,
            imported_name,
            type_name,
            kind,
            span: span(node),
        }))
    }
}

fn fixture_annotation(raw: &str) -> &str {
    let Some((head, tail)) = raw.split_once('[') else {
        return raw;
    };
    if matches!(
        head.rsplit('.').next(),
        Some(
            "Iterator"
                | "Iterable"
                | "Generator"
                | "AsyncIterator"
                | "AsyncIterable"
                | "AsyncGenerator"
        )
    ) {
        tail.trim_end_matches(']')
            .split(',')
            .next()
            .unwrap_or(raw)
            .trim()
    } else {
        raw
    }
}
