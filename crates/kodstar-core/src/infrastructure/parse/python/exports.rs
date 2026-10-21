// Purpose: Python 모듈 __all__의 정적 이름·갱신·동적 상태 사실 추출.
// Pattern: Simple Module.
// Usage: Python 노드 참조 처리에서 module scope의 명시적 export 변경을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/references.rs, crates/kodstar-core/src/application/analysis/resolve/python/exports.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq)]
enum Sequence {
    List,
    Tuple,
}

fn sequence(node: Node<'_>, source: &[u8], depth: usize) -> Option<Sequence> {
    if depth >= 64 {
        return None;
    }
    match node.kind() {
        "list" => Some(Sequence::List),
        "tuple" => Some(Sequence::Tuple),
        "parenthesized_expression" => sequence(node.named_child(0)?, source, depth + 1),
        "binary_operator"
            if node
                .child_by_field_name("operator")
                .is_some_and(|n| text(n, source) == "+") =>
        {
            let left = sequence(node.child_by_field_name("left")?, source, depth + 1)?;
            let right = sequence(node.child_by_field_name("right")?, source, depth + 1)?;
            (left == right).then_some(left)
        }
        _ => None,
    }
}

fn all_target(node: Node<'_>, source: &[u8], depth: usize) -> bool {
    if depth >= 64 {
        return true;
    }
    match node.kind() {
        "identifier" => text(node, source) == "__all__",
        "attribute" => node
            .child_by_field_name("object")
            .is_some_and(|n| all_target(n, source, depth + 1)),
        "subscript" => node
            .child_by_field_name("value")
            .is_some_and(|n| all_target(n, source, depth + 1)),
        "expression_list" | "pattern_list" | "tuple" | "list" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .any(|n| all_target(n, source, depth + 1))
        }
        _ => false,
    }
}

fn conditional(node: Node<'_>) -> bool {
    let mut parent = node.parent();
    while let Some(current) = parent {
        if matches!(
            current.kind(),
            "if_statement"
                | "for_statement"
                | "while_statement"
                | "try_statement"
                | "with_statement"
                | "match_statement"
        ) {
            return true;
        }
        parent = current.parent();
    }
    false
}

fn escapes(mut node: Node<'_>, source: &[u8]) -> bool {
    if node.kind() != "identifier" || text(node, source) != "__all__" {
        return false;
    }
    let mut parent = node.parent();
    while let Some(wrapper) = parent.filter(|n| n.kind() == "parenthesized_expression") {
        node = wrapper;
        parent = node.parent();
    }
    let Some(parent) = parent else {
        return false;
    };
    match parent.kind() {
        "assignment" => parent
            .child_by_field_name("right")
            .is_some_and(|n| n.id() == node.id()),
        "named_expression" => parent
            .child_by_field_name("value")
            .is_some_and(|n| n.id() == node.id()),
        "argument_list"
        | "list"
        | "tuple"
        | "set"
        | "pair"
        | "list_comprehension"
        | "set_comprehension"
        | "generator_expression" => true,
        _ => false,
    }
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn python_exports(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Python || context.function_scope != 0 {
            return Ok(());
        }
        let assignment = context.scope == 0
            && matches!(node.kind(), "assignment" | "augmented_assignment")
            && node
                .child_by_field_name("left")
                .is_some_and(|n| n.kind() == "identifier" && text(n, self.source) == "__all__");
        let deleted = context.scope == 0
            && node.kind() == "delete_statement"
            && node
                .named_child(0)
                .is_some_and(|n| text(n, self.source) == "__all__");
        let call_mutation = node.kind() == "call"
            && node
                .child_by_field_name("function")
                .filter(|n| n.kind() == "attribute")
                .and_then(|n| n.child_by_field_name("object"))
                .is_some_and(|n| n.kind() == "identifier" && text(n, self.source) == "__all__");
        let write_mutation = matches!(node.kind(), "assignment" | "augmented_assignment")
            && node.child_by_field_name("left").is_some_and(|n| {
                (context.scope == 0 || n.kind() != "identifier") && all_target(n, self.source, 0)
            });
        let delete_mutation = node.kind() == "delete_statement"
            && node.named_child(0).is_some_and(|n| {
                (context.scope == 0 || n.kind() != "identifier") && all_target(n, self.source, 0)
            });
        let mutation =
            call_mutation || write_mutation || delete_mutation || escapes(node, self.source);
        if !assignment && !deleted && !mutation {
            return Ok(());
        }
        let mut kind = "python_all_unknown";
        if !conditional(node) {
            if deleted {
                kind = "python_all_absent";
            } else if assignment && let Some(value) = node.child_by_field_name("right") {
                let append = node.kind() == "augmented_assignment";
                let plus = !append
                    || node
                        .child_by_field_name("operator")
                        .is_some_and(|n| text(n, self.source) == "+=");
                if plus && self.python_export_sequence(value, node.start_byte())? {
                    kind = if append {
                        "python_all_append"
                    } else {
                        "python_all_reset"
                    };
                }
            }
        }
        (self.emit)(Fact::Binding(Binding {
            scope: 0,
            name: "@python-all-state".into(),
            target: None,
            module: None,
            imported_name: None,
            type_name: None,
            kind,
            span: span(node),
        }))
    }

    fn python_export_sequence(&mut self, root: Node<'_>, assignment: usize) -> Result<bool> {
        if sequence(root, self.source, 0).is_none() {
            return Ok(false);
        }
        let mut cursor = root.walk();
        let mut known = true;
        loop {
            let node = cursor.node();
            if node.id() != root.id()
                && node
                    .parent()
                    .is_some_and(|n| matches!(n.kind(), "list" | "tuple"))
                && !matches!(node.kind(), "string")
                && node.is_named()
                && !node.is_extra()
            {
                known = false;
            }
            let descend = match node.kind() {
                "list" | "tuple"
                    if node
                        .parent()
                        .is_some_and(|n| matches!(n.kind(), "list" | "tuple")) =>
                {
                    false
                }
                "list" | "tuple" | "parenthesized_expression" => true,
                "binary_operator"
                    if node
                        .child_by_field_name("operator")
                        .is_some_and(|n| text(n, self.source) == "+") =>
                {
                    true
                }
                "string" => {
                    if let Some(content) =
                        crate::infrastructure::parse::python::literals::plain_string(
                            node,
                            self.source,
                        )
                    {
                        let name = self.bounded_text(content)?;
                        (self.emit)(Fact::Binding(Binding {
                            scope: 0,
                            name: "@python-all-name".into(),
                            target: None,
                            module: Some(assignment.to_string()),
                            imported_name: Some(name),
                            type_name: None,
                            kind: "python_all_entry",
                            span: span(node),
                        }))?;
                    } else {
                        known = false;
                    }
                    false
                }
                _ if !node.is_named() || node.is_extra() => false,
                _ => {
                    known = false;
                    false
                }
            };
            if descend && cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return Ok(known);
                }
            }
        }
    }
}
