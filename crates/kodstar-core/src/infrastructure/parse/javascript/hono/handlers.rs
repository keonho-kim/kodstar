// Purpose: inline handler와 첫 매개변수의 Hono 등록 근거 추출.
// Pattern: Syntax Adapter.
// Usage: 정의 추출기가 할당한 함수 local ID와 원래 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/javascript/hono/mod.rs, crates/kodstar-core/src/infrastructure/parse/definitions.rs.

use super::shape;
use crate::domain::{diagnostics::Result, languages::Language, web::Route};
use crate::infrastructure::parse::{Extractor, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn hono_handler(
        &mut self,
        node: Node<'_>,
        id: u32,
        scope: u32,
        source: u32,
    ) -> Result<()> {
        if !matches!(self.language, Language::JavaScript | Language::TypeScript) {
            return Ok(());
        }
        let Some(arguments) = node.parent().filter(|p| p.kind() == "arguments") else {
            return Ok(());
        };
        let Some(call) = arguments.parent() else {
            return Ok(());
        };
        let Some((app, method, path, first)) = shape::route(call, self.source) else {
            return Ok(());
        };
        let mut cursor = arguments.walk();
        let index = arguments
            .named_children(&mut cursor)
            .position(|n| n.id() == node.id());
        if let Some(index) = index.filter(|i| *i >= first) {
            self.hono_registration(
                Route {
                    app,
                    method,
                    path,
                    handler: None,
                    last: index + 1 == arguments.named_child_count(),
                },
                Some(id),
                scope,
                source,
                node,
            )?;
        }
        Ok(())
    }
}

impl Extractor<'_> {
    pub(super) fn hono_parameter(&mut self, node: Node<'_>, id: u32) -> Result<()> {
        if !self.hono_contexts.insert(id) {
            return Ok(());
        }
        if self.hono_contexts.len() > self.fact_limit / 32 {
            return Err(crate::domain::diagnostics::Error::new(
                crate::domain::diagnostics::ErrorKind::Limit,
                "Hono context workspace exceeded",
            ));
        }
        let parameter = node.child_by_field_name("parameter").or_else(|| {
            node.child_by_field_name("parameters")
                .and_then(|p| p.named_child(0))
        });
        if let Some(mut parameter) = parameter {
            if parameter.kind() == "required_parameter" {
                parameter = parameter
                    .child_by_field_name("pattern")
                    .unwrap_or(parameter);
            }
            if parameter.kind() == "identifier" {
                self.hono_metadata(
                    "hono_context",
                    "@hono-context",
                    text(parameter, self.source).into(),
                    Some(id),
                    id,
                    parameter,
                )?;
            }
        }
        Ok(())
    }
}
