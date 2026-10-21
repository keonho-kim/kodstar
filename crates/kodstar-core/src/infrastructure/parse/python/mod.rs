// Purpose: Python scope 지시와 구조 분해·반복·with·except의 lexical binding 추출.
// Pattern: Simple Module.
// Usage: binding 추출기가 Python 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/bindings.rs, crates/kodstar-core/src/application/analysis/resolve/python/mod.rs.

pub(super) mod comprehensions;
pub(super) mod exports;
pub(super) mod fixtures;
pub(super) mod imports;
pub(super) mod literals;
pub(super) mod members;

pub(super) mod decorators;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn python_binding(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<bool> {
        if self.language != Language::Python {
            return Ok(false);
        }
        let (root, kind) = match node.kind() {
            "global_statement" => (Some(node), "python_global"),
            "nonlocal_statement" => (Some(node), "python_nonlocal"),
            "for_statement" | "for_in_clause" | "augmented_assignment" => {
                (node.child_by_field_name("left"), "local")
            }
            "named_expression" => (node.child_by_field_name("name"), "local"),
            "as_pattern_target" => (Some(node), "local"),
            "delete_statement" => (Some(node), "local"),
            "assignment"
                if node
                    .child_by_field_name("left")
                    .is_some_and(|n| n.kind() != "identifier") =>
            {
                (node.child_by_field_name("left"), "local")
            }
            _ => return Ok(false),
        };
        let Some(root) = root else { return Ok(true) };
        let scope = if node.kind() == "named_expression" && context.python_comprehension.is_some() {
            context.function_scope
        } else {
            context.scope
        };
        let mut cursor = root.walk();
        loop {
            let current = cursor.node();
            if current.kind() == "identifier" {
                let name = self.bounded_text(text(current, self.source))?;
                (self.emit)(Fact::Binding(Binding {
                    scope,
                    name,
                    target: None,
                    module: None,
                    imported_name: None,
                    type_name: None,
                    kind,
                    span: span(current),
                }))?;
            } else if !matches!(current.kind(), "attribute" | "subscript")
                && cursor.goto_first_child()
            {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return Ok(true);
                }
            }
        }
    }
}
