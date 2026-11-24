// Purpose: 오류 구간의 투명 순회·안전한 선언 보존과 원문 진단 위치 소유.
// Pattern: Recovery Policy.
// Usage: 파일 추출과 반복 visitor가 같은 구문 복구 규칙을 사용.
// Related: crates/kodstar-core/src/infrastructure/parse/walk.rs, crates/kodstar-core/src/infrastructure/parse/session.rs, crates/kodstar-core/src/infrastructure/parse/definitions.rs.

use super::{ParseSession, SyntaxConfig, child_kind, span, syntax::name_node};
use crate::domain::source::SourceSpan;
use tree_sitter::{Node, Tree};

#[derive(Clone, Copy)]
pub(super) enum Action {
    Enter,
    HeaderOnly,
    Transparent,
    Skip,
}

pub(super) fn action(node: Node<'_>, config: &SyntaxConfig, opaque: bool, special: bool) -> Action {
    if node.is_missing() {
        return Action::Skip;
    }
    if node.is_error() {
        return Action::Transparent;
    }
    let declaration =
        special || config.classes.contains(&node.kind()) || config.functions.contains(&node.kind());
    if declaration && node.has_error() {
        return if header_is_intact(node, special) {
            if scope_is_closed(node) {
                Action::Enter
            } else {
                Action::HeaderOnly
            }
        } else {
            Action::Skip
        };
    }
    if opaque && !declaration {
        return Action::Transparent;
    }
    if node.has_error() && !declaration && !container(node.kind()) {
        return Action::Transparent;
    }
    Action::Enter
}

fn scope_is_closed(node: Node<'_>) -> bool {
    if node
        .child(node.child_count().saturating_sub(1))
        .is_some_and(|last| last.is_missing())
    {
        return false;
    }
    let Some(body) = body(node) else {
        return false;
    };
    let last = if body.kind() == "{" {
        node.child(node.child_count().saturating_sub(1))
    } else {
        body.child(body.child_count().saturating_sub(1))
    };
    if last.is_some_and(|last| last.is_missing()) {
        return false;
    }
    if body.kind() == "{" {
        return last.is_some_and(|last| last.kind() == "}");
    }
    if body.child(0).is_some_and(|first| first.kind() == "{") {
        return body
            .child(body.child_count().saturating_sub(1))
            .is_some_and(|last| last.kind() == "}" && !last.is_missing());
    }
    true
}

fn header_is_intact(node: Node<'_>, special: bool) -> bool {
    let named = name_node(node).is_some_and(|name| {
        !name.has_error() && !name.is_missing() && name.start_byte() != name.end_byte()
    });
    if !named && !special {
        return false;
    }
    let Some(body) = body(node) else {
        return false;
    };
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .all(|child| !child.has_error() || child.start_byte() >= body.start_byte())
}

fn body(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("body")
        .or_else(|| {
            child_kind(
                node,
                &[
                    "class_body",
                    "enum_body",
                    "interface_body",
                    "declaration_list",
                    "block",
                    "statement_block",
                    "compound_statement",
                    "function_body",
                    "do_block",
                    "{",
                ],
            )
        })
        .or_else(|| {
            node.child_by_field_name("type")
                .filter(|n| matches!(n.kind(), "struct_type" | "interface_type"))
        })
}

fn container(kind: &str) -> bool {
    matches!(
        kind,
        "source_file"
            | "program"
            | "module"
            | "compilation_unit"
            | "translation_unit"
            | "class_body"
            | "enum_body"
            | "interface_body"
            | "declaration_list"
            | "class_member_declarations"
            | "class_body_declarations"
            | "block"
            | "statement_block"
            | "compound_statement"
            | "function_body"
            | "do_block"
            | "body"
            | "body_statement"
            | "statements"
    ) || kind.starts_with("preproc_")
}

pub(super) fn first_error(tree: &Tree) -> Option<SourceSpan> {
    if !tree.root_node().has_error() {
        return None;
    }
    let mut node = tree.root_node();
    for _ in 0..1024 {
        let mut children = node.walk();
        let child = node
            .children(&mut children)
            .find(|child| child.has_error() || child.is_missing() || child.is_error());
        if let Some(child) = child {
            node = child;
        } else {
            return Some(span(node));
        }
    }
    Some(span(node))
}

impl ParseSession {
    pub(super) fn record_partial(&mut self, tree: &Tree) {
        self.partial = earliest(self.partial, first_error(tree));
    }

    pub(crate) fn remap_partial(
        &mut self,
        earlier: Option<SourceSpan>,
        map: impl FnOnce(SourceSpan) -> SourceSpan,
    ) {
        self.partial = earliest(earlier, self.partial.take().map(map));
    }
}

fn earliest(first: Option<SourceSpan>, next: Option<SourceSpan>) -> Option<SourceSpan> {
    match (first, next) {
        (Some(a), Some(b)) => Some(if a.start_byte <= b.start_byte { a } else { b }),
        (a, b) => a.or(b),
    }
}
