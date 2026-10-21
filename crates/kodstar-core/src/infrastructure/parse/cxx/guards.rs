// Purpose: 파일 전체를 감싸는 정형 C/C++ header guard의 구조적 사실 추출.
// Pattern: Simple Module.
// Usage: header AST 진입이 guard 후보를 전달하며 비교 시 조건과 구분.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/application/analysis/resolve/cxx/mod.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::infrastructure::parse::{Extractor, child_kind, span, text};
use tree_sitter::Node;

fn unwrapped(mut node: Node<'_>) -> Option<Node<'_>> {
    for _ in 0..32 {
        if node.kind() != "parenthesized_expression" {
            return Some(node);
        }
        node = node.named_child(0)?;
    }
    None
}

fn guard_name<'tree>(node: Node<'tree>, source: &[u8]) -> Option<Node<'tree>> {
    if node.kind() == "preproc_ifdef" {
        let mut cursor = node.walk();
        return node
            .children(&mut cursor)
            .any(|n| n.kind() == "#ifndef")
            .then(|| node.child_by_field_name("name"))
            .flatten();
    }
    let condition = unwrapped(node.child_by_field_name("condition")?)?;
    if condition.kind() != "unary_expression"
        || !condition
            .child_by_field_name("operator")
            .is_some_and(|n| text(n, source) == "!")
    {
        return None;
    }
    let argument = unwrapped(condition.child_by_field_name("argument")?)?;
    if argument.kind() != "preproc_defined" {
        return None;
    }
    child_kind(argument, &["identifier"])
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn cxx_header_guard(
        &mut self,
        node: Node<'_>,
    ) -> Result<()> {
        if !self.cxx_header
            || !matches!(node.kind(), "preproc_if" | "preproc_ifdef")
            || node.child_by_field_name("alternative").is_some()
        {
            return Ok(());
        }
        let Some(parent) = node.parent().filter(|n| n.kind() == "translation_unit") else {
            return Ok(());
        };
        let mut cursor = parent.walk();
        if parent
            .named_children(&mut cursor)
            .any(|n| !n.is_extra() && n.id() != node.id())
        {
            return Ok(());
        }
        let Some(name) = guard_name(node, self.source) else {
            return Ok(());
        };
        let condition = node.child_by_field_name("condition");
        let mut cursor = node.walk();
        let definition = node.named_children(&mut cursor).find(|n| {
            !n.is_extra() && n.id() != name.id() && !condition.is_some_and(|c| c.id() == n.id())
        });
        if !definition.is_some_and(|n| {
            n.kind() == "preproc_def"
                && n.child_by_field_name("name")
                    .is_some_and(|n| text(n, self.source) == text(name, self.source))
        }) {
            return Ok(());
        }
        let guard = self.bounded_text(text(name, self.source))?;
        let condition = self.native_condition(node)?;
        (self.emit)(Fact::Binding(Binding {
            scope: 0,
            name: "@cxx-header-guard".into(),
            target: None,
            module: Some(guard),
            imported_name: None,
            type_name: condition,
            kind: "cxx_header_guard",
            span: span(node),
        }))
    }
}
