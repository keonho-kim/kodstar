// Purpose: JSX 응답의 Context receiver와 렌더링 컴포넌트 근거 추출.
// Pattern: Syntax Adapter.
// Usage: JS/TS JSX 태그 방문자가 현재 함수 소유 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/javascript/hono/mod.rs, crates/kodstar-core/src/infrastructure/parse/javascript/mod.rs.

use super::shape;
use crate::domain::{diagnostics::Result, relations::RelationKind};
use crate::infrastructure::parse::{Context, Extractor};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn hono_response(
        &mut self,
        tag: Node<'_>,
        name: &str,
        context: &Context,
    ) -> Result<()> {
        let mut current = tag;
        for _ in 0..64 {
            let Some(parent) = current.parent() else {
                break;
            };
            if matches!(
                parent.kind(),
                "arrow_function"
                    | "function_expression"
                    | "function_declaration"
                    | "method_definition"
            ) {
                break;
            }
            if let Some((receiver, method, arguments)) = shape::call(parent, self.source)
                && matches!(method, "html" | "render")
                && receiver.kind() == "identifier"
                && arguments.named_child(0).is_some_and(|n| {
                    n.start_byte() <= tag.start_byte() && n.end_byte() >= tag.end_byte()
                })
            {
                self.hono_response_parameter(parent, context.source)?;
                self.hono_metadata(
                    "hono_response",
                    "@hono-response",
                    crate::infrastructure::parse::text(receiver, self.source).into(),
                    Some(context.source),
                    context.scope,
                    tag,
                )?;
                return self.usage(
                    context,
                    name.into(),
                    RelationKind::Renders,
                    crate::infrastructure::parse::span(tag),
                );
            }
            current = parent;
        }
        Ok(())
    }
}

impl Extractor<'_> {
    fn hono_response_parameter(&mut self, node: Node<'_>, id: u32) -> Result<()> {
        let mut owner = Some(node);
        for _ in 0..64 {
            let Some(function) = owner else {
                break;
            };
            if matches!(
                function.kind(),
                "arrow_function"
                    | "function_expression"
                    | "function_declaration"
                    | "method_definition"
            ) {
                self.hono_parameter(function, id)?;
                break;
            }
            owner = function.parent();
        }
        Ok(())
    }
    pub(super) fn hono_html(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        if !self.htmx {
            return Ok(());
        }
        let Some((receiver, method, arguments)) = shape::call(node, self.source) else {
            return Ok(());
        };
        if !matches!(method, "html" | "render") || receiver.kind() != "identifier" {
            return Ok(());
        }
        let Some(argument) = arguments.named_child(0) else {
            return Ok(());
        };
        if shape::literal(argument, self.source).is_none() {
            return Ok(());
        }
        let range = argument.start_byte() + 1..argument.end_byte().saturating_sub(1);
        self.hono_response_parameter(node, context.source)?;
        let name = crate::infrastructure::parse::text(receiver, self.source).to_owned();
        let origin = crate::domain::source::SourceSpan::at(self.source, range.start, range.start);
        let owner = crate::infrastructure::formats::htmx::Owner {
            language: self.language,
            source: context.source,
            scope: context.scope,
            disabled: false,
        };
        crate::infrastructure::formats::htmx::literal(
            &self.source[range],
            origin,
            owner,
            self.cancel,
            &mut |fact| {
                if let crate::domain::facts::Fact::Use(usage) = &fact {
                    (self.emit)(crate::domain::facts::Fact::Binding(
                        crate::domain::facts::Binding {
                            scope: context.scope,
                            name: "@hono-response".into(),
                            target: Some(context.source),
                            module: Some(name.clone()),
                            imported_name: None,
                            type_name: None,
                            kind: "hono_response",
                            span: usage.span,
                        },
                    ))?;
                }
                (self.emit)(fact)
            },
        )
    }
}
