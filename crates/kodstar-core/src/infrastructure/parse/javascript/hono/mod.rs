// Purpose: Hono 앱 생성·라우트 등록과 재할당의 파일 내 구문 근거 추출.
// Pattern: Syntax Adapter.
// Usage: JS/TS 공통 walker가 노드·lexical 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/javascript/hono/shape.rs, crates/kodstar-core/src/application/analysis/resolve/http/mod.rs.

mod handlers;
mod responses;
mod shape;

use crate::domain::{
    diagnostics::Result,
    facts::{Binding, Fact},
    languages::Language,
    relations::RelationKind,
    web::{Mount, Route},
};
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn hono(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if !matches!(self.language, Language::JavaScript | Language::TypeScript) {
            return Ok(());
        }
        if node.kind() == "variable_declarator"
            && let (Some(name), Some(value)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("value"),
            )
            && name.kind() == "identifier"
            && let Some(app) =
                shape::app(value, self.source, 0).filter(|_| value.kind() != "identifier")
        {
            self.hono_metadata(
                "hono_app",
                &format!("@hono-app:{}", text(name, self.source)),
                encode(&app)?,
                None,
                context.scope,
                name,
            )?;
            if crate::infrastructure::parse::javascript::imports::exported(node) {
                let reference = crate::domain::web::App {
                    name: text(name, self.source).into(),
                    constructor: false,
                    prefix: String::new(),
                };
                self.hono_metadata(
                    "hono_export",
                    &format!("@hono-export:{}", text(name, self.source)),
                    encode(&reference)?,
                    None,
                    context.scope,
                    name,
                )?;
            }
        }
        if matches!(
            node.kind(),
            "assignment_expression" | "augmented_assignment_expression" | "update_expression"
        ) && let Some(name) = node
            .child_by_field_name("left")
            .or_else(|| node.child_by_field_name("argument"))
            && name.kind() == "identifier"
        {
            self.hono_metadata(
                "hono_write",
                &format!("@hono-write:{}", text(name, self.source)),
                String::new(),
                None,
                context.scope,
                name,
            )?;
        }
        self.hono_html(node, context)?;
        let Some((receiver, method, arguments)) = shape::call(node, self.source) else {
            return Ok(());
        };
        if method == "route"
            && let (Some(parent), Some(child), Some(path)) = (
                shape::app(receiver, self.source, 0),
                arguments
                    .named_child(1)
                    .and_then(|n| shape::app(n, self.source, 0)),
                arguments
                    .named_child(0)
                    .and_then(|n| shape::literal(n, self.source)),
            )
        {
            self.hono_metadata(
                "hono_mount",
                "@hono-mount",
                encode(&Mount {
                    parent,
                    child,
                    path,
                })?,
                None,
                context.scope,
                node,
            )?;
        }
        let Some((app, method, path, first)) = shape::route(node, self.source) else {
            return Ok(());
        };
        let mut cursor = arguments.walk();
        for (index, handler) in arguments
            .named_children(&mut cursor)
            .enumerate()
            .skip(first)
        {
            if matches!(
                handler.kind(),
                "arrow_function"
                    | "function_expression"
                    | "object"
                    | "array"
                    | "string"
                    | "number"
                    | "true"
                    | "false"
                    | "null"
            ) {
                continue;
            }
            let route = Route {
                app: app.clone(),
                method: method.clone(),
                path: path.clone(),
                handler: matches!(handler.kind(), "identifier" | "member_expression")
                    .then(|| self.bounded_text(text(handler, self.source)))
                    .transpose()?,
                last: index + 1 == arguments.named_child_count(),
            };
            self.hono_registration(route, None, context.scope, context.source, handler)?;
        }
        Ok(())
    }

    fn hono_metadata(
        &mut self,
        kind: &'static str,
        name: &str,
        data: String,
        target: Option<u32>,
        scope: u32,
        node: Node<'_>,
    ) -> Result<()> {
        let name = self.bounded_text(name)?;
        let data = self.bounded_text(&data)?;
        (self.emit)(Fact::Binding(Binding {
            scope,
            name,
            target,
            module: Some(data),
            imported_name: None,
            type_name: None,
            kind,
            span: span(node),
        }))
    }

    fn hono_registration(
        &mut self,
        route: Route,
        target: Option<u32>,
        scope: u32,
        source: u32,
        node: Node<'_>,
    ) -> Result<()> {
        let expression = format!("{} {}", route.method, route.path);
        self.hono_metadata(
            "hono_route",
            "@hono-route",
            encode(&route)?,
            target,
            scope,
            node,
        )?;
        (self.emit)(Fact::Use(crate::domain::facts::SyntaxUse {
            source,
            scope,
            expression,
            relation: RelationKind::Handles,
            span: span(node),
            condition: None,
            type_only: false,
        }))
    }
}

fn encode(value: &impl serde::Serialize) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|e| crate::domain::diagnostics::Error::caused_by("encode Hono syntax evidence", e))
}
