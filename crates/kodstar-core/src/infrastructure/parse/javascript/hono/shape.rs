// Purpose: Hono 후보 호출의 정적 메서드·경로·앱 표현을 AST에서 읽음.
// Pattern: Syntax Adapter.
// Usage: Hono 등록·인스턴스 추출기가 같은 AST 형태를 사용.
// Related: crates/kodstar-core/src/domain/web/mod.rs, crates/kodstar-core/src/infrastructure/parse/javascript/hono/mod.rs.

use crate::domain::web::{self, App};
use crate::infrastructure::parse::text;
use tree_sitter::Node;

pub(super) fn literal(node: Node<'_>, source: &[u8]) -> Option<String> {
    if !matches!(node.kind(), "string" | "template_string") {
        return None;
    }
    let value = text(node, source);
    if value.len() < 2 || value.contains('\\') || value.contains("${") {
        return None;
    }
    Some(value[1..value.len() - 1].into())
}

pub(super) fn call<'a, 'b>(
    node: Node<'a>,
    source: &'b [u8],
) -> Option<(Node<'a>, &'b str, Node<'a>)> {
    if node.kind() != "call_expression" {
        return None;
    }
    let function = node.child_by_field_name("function")?;
    if function.kind() != "member_expression" {
        return None;
    }
    Some((
        function.child_by_field_name("object")?,
        text(function.child_by_field_name("property")?, source),
        node.child_by_field_name("arguments")?,
    ))
}

pub(super) fn app(node: Node<'_>, source: &[u8], depth: usize) -> Option<App> {
    if depth >= 32 {
        return None;
    }
    match node.kind() {
        "identifier" => Some(App {
            name: text(node, source).into(),
            constructor: false,
            prefix: String::new(),
        }),
        "new_expression" => {
            if node
                .child_by_field_name("arguments")
                .is_some_and(|a| a.named_child_count() > 0)
            {
                return None;
            }
            let constructor = node.child_by_field_name("constructor")?;
            if !matches!(constructor.kind(), "identifier" | "member_expression") {
                return None;
            }
            Some(App {
                name: text(constructor, source).into(),
                constructor: true,
                prefix: String::new(),
            })
        }
        "parenthesized_expression" | "as_expression" | "non_null_expression" => {
            app(node.named_child(0)?, source, depth + 1)
        }
        "call_expression" => {
            let (receiver, method, arguments) = call(node, source)?;
            let mut result = app(receiver, source, depth + 1)?;
            if method == "basePath" {
                let prefix = literal(arguments.named_child(0)?, source)?;
                result.prefix = web::join(&result.prefix, &prefix)?;
            } else if web::method(method).is_none() && !matches!(method, "route" | "use") {
                return None;
            }
            Some(result)
        }
        _ => None,
    }
}

pub(super) fn route(node: Node<'_>, source: &[u8]) -> Option<(App, String, String, usize)> {
    let (receiver, method, arguments) = call(node, source)?;
    if method == "use" {
        let first = arguments.named_child(0)?;
        let (path, skip) = if let Some(path) = literal(first, source) {
            (if path == "*" { "/*".into() } else { path }, 1)
        } else if arguments.named_child_count() == 1
            || matches!(first.kind(), "arrow_function" | "function_expression")
        {
            ("/*".into(), 0)
        } else {
            ("<dynamic>".into(), 0)
        };
        return Some((app(receiver, source, 0)?, "USE".into(), path, skip));
    }
    let (method, path, first) = if method == "on" {
        (
            web::method(&literal(arguments.named_child(0)?, source)?)?,
            arguments.named_child(1)?,
            2,
        )
    } else {
        (web::method(method)?, arguments.named_child(0)?, 1)
    };
    Some((
        app(receiver, source, 0)?,
        method.into(),
        literal(path, source)
            .map(|p| if p == "*" { "/*".into() } else { p })
            .unwrap_or_else(|| "<dynamic>".into()),
        first,
    ))
}
