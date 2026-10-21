// Purpose: Go method set 비교용 인수 수와 interface embedding 사실 추출.
// Pattern: Simple Module.
// Usage: 선언 순회가 Go 함수·interface 항목을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/application/analysis/resolve/go/interfaces.rs.

pub(super) mod imports;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn go_signature(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Go {
            return Ok(());
        }
        let parameters = arity(node.child_by_field_name("parameters"));
        let results = arity(node.child_by_field_name("result"));
        (self.emit)(Fact::Binding(Binding {
            scope: context.scope,
            name: "@signature".into(),
            target: Some(context.source),
            module: Some(format!("{parameters}:{results}")),
            imported_name: None,
            type_name: None,
            kind: "go_signature",
            span: span(node),
        }))
    }

    pub(in crate::infrastructure::parse) fn go_embedding(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Go
            || node.kind() != "type_elem"
            || !node.parent().is_some_and(|p| p.kind() == "interface_type")
        {
            return Ok(());
        }
        let value = text(node, self.source);
        if value.contains(['~', '|']) || value == "comparable" {
            return (self.emit)(Fact::Binding(Binding {
                scope: context.scope,
                name: "@constraint".into(),
                target: Some(context.source),
                module: None,
                imported_name: None,
                type_name: None,
                kind: "go_constraint",
                span: span(node),
            }));
        }
        self.usage(
            context,
            self.bounded_text(value)?,
            RelationKind::MixesIn,
            span(node),
        )
    }
}

fn arity(node: Option<Node<'_>>) -> usize {
    let Some(node) = node else { return 0 };
    if node.kind() != "parameter_list" {
        return 1;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|n| {
            matches!(
                n.kind(),
                "parameter_declaration" | "variadic_parameter_declaration"
            )
        })
        .map(|n| {
            let mut names = n.walk();
            n.children_by_field_name("name", &mut names).count().max(1)
        })
        .sum()
}
