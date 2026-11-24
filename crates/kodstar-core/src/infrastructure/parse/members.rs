// Purpose: enum case의 원래 이름·소유·containment 사실 추출.
// Pattern: Simple Module.
// Usage: 선언 순회가 현재 노드와 enum 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/application/analysis/resolve/inputs.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Definition, Fact, SyntaxUse};
use crate::domain::relations::RelationKind;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn enum_member(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        let candidate = matches!(
            node.kind(),
            "enum_variant"
                | "enumerator"
                | "enum_constant"
                | "enum_member_declaration"
                | "enum_assignment"
                | "enum_case"
                | "enum_entry"
                | "simple_enum_case"
                | "full_enum_case"
        ) || node.kind() == "property_identifier"
            && node.parent().is_some_and(|p| p.kind() == "enum_body")
            || node.kind() == "container_field"
                && node
                    .parent()
                    .is_some_and(|p| p.kind() == "enum_declaration");
        let candidate = candidate
            || self.language == crate::domain::languages::Language::Zig
                && crate::infrastructure::parse::zig::case(node);
        let candidate = candidate
            || node.kind() == "record_field"
                && node.parent().is_some_and(|p| p.kind() == "record_decl");
        if !candidate {
            return Ok(());
        }
        if node.kind() == "enum_entry" {
            let mut cursor = node.walk();
            for name in node
                .named_children(&mut cursor)
                .filter(|n| matches!(n.kind(), "simple_identifier" | "identifier"))
            {
                self.case_definition(name, node, context)?;
            }
        } else if let Some(name) = (matches!(node.kind(), "property_identifier" | "identifier"))
            .then_some(node)
            .or_else(|| node.child_by_field_name("name"))
            .or_else(|| {
                child_kind(
                    node,
                    &[
                        "identifier",
                        "field_identifier",
                        "type_identifier",
                        "name",
                        "simple_identifier",
                    ],
                )
            })
        {
            self.case_definition(name, node, context)?;
        }
        Ok(())
    }

    fn case_definition(&mut self, name: Node<'_>, node: Node<'_>, context: &Context) -> Result<()> {
        let local_id = self.allocate()?;
        let name = self.bounded_text(text(name, self.source))?;
        let qualified_name = context.prefix.child(&name, self.fact_limit)?.render()?;
        (self.emit)(Fact::Definition(Definition {
            local_id,
            owner: Some(context.owner),
            scope: context.scope,
            name: name.clone(),
            qualified_name,
            kind: SymbolKind::Field,
            signature: String::new(),
            condition: context.condition.as_deref().map(str::to_owned),
            span: span(node),
            exported: true,
        }))?;
        (self.emit)(Fact::Binding(Binding {
            scope: context.scope,
            name,
            target: Some(local_id),
            module: None,
            imported_name: None,
            type_name: None,
            kind: "enum_case",
            span: span(node),
        }))?;
        self.rust_constructor(node, local_id, context.scope)?;
        (self.emit)(Fact::Use(SyntaxUse {
            source: context.owner,
            scope: context.scope,
            expression: format!("@local:{local_id}"),
            relation: RelationKind::Contains,
            span: span(node),
            condition: context.condition.as_deref().map(str::to_owned),
            type_only: false,
        }))
    }
}
