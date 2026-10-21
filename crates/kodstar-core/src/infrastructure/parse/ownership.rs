// Purpose: 명시적 impl·receiver 타입과 분리 메서드 문맥 추출.
// Pattern: Simple Module.
// Usage: 선언 추출기가 현재 노드와 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/application/analysis/resolve/owners.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact, Scope, SyntaxUse};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Context, Extractor, NamePath, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn implementation(&mut self, node: Node<'_>, context: &mut Context) -> Result<()> {
        let owner = match (self.language, node.kind()) {
            (Language::Rust, "impl_item") => node.child_by_field_name("type"),
            (Language::Go, "method_declaration") => node
                .child_by_field_name("receiver")
                .and_then(|n| n.named_child(0))
                .and_then(|n| n.child_by_field_name("type")),
            (Language::Swift, "class_declaration") if extension(node) => {
                node.child_by_field_name("name")
            }
            _ => None,
        };
        let Some(owner) = owner else {
            return Ok(());
        };
        let owner = text(owner, self.source)
            .trim_start_matches(['*', '&'])
            .split(['<', '['])
            .next()
            .unwrap_or("")
            .trim()
            .trim_end_matches("::");
        let owner = if owner.contains("::") {
            NamePath::root(owner, self.fact_limit)?
        } else {
            context.prefix.child(owner, self.fact_limit)?
        };
        let scope = self.allocate()?;
        (self.emit)(Fact::Scope(Scope {
            local_id: scope,
            parent: Some(context.scope),
            span: span(node),
        }))?;
        context.scope = scope;
        context.prefix = owner.clone();
        context.type_name = Some(owner.clone());
        context.implementation_owner = Some(owner.clone());
        context.member_scope = true;
        if self.language != Language::Go {
            (self.emit)(Fact::Binding(Binding {
                scope,
                name: "Self".into(),
                target: None,
                module: None,
                imported_name: Some(owner.render()?),
                type_name: None,
                kind: "alias",
                span: span(node),
            }))?;
        }
        self.type_parameters(node, context)?;
        if self.language == Language::Rust {
            (self.emit)(Fact::Binding(Binding {
                scope,
                name: "Self".into(),
                target: None,
                module: None,
                imported_name: Some(owner.render()?),
                type_name: None,
                kind: "rust_self_constructor",
                span: span(node),
            }))?;
        }
        if self.language == Language::Rust
            && let Some(contract) = node.child_by_field_name("trait")
        {
            let expression = format!(
                "@impl:{}#{}",
                owner.render()?,
                self.reference_text(contract)?
            );
            (self.emit)(Fact::Use(SyntaxUse {
                source: context.source,
                scope,
                expression,
                relation: crate::domain::relations::RelationKind::Implements,
                span: span(contract),
                condition: context.condition.as_deref().map(str::to_owned),
                type_only: false,
            }))?;
        }
        Ok(())
    }

    pub(super) fn owner_hint(
        &mut self,
        method: u32,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if let Some(owner) = &context.implementation_owner {
            (self.emit)(Fact::Binding(Binding {
                scope: context.scope,
                name: "@owner".into(),
                target: Some(method),
                module: None,
                imported_name: Some(owner.render()?),
                type_name: None,
                kind: "owner_hint",
                span: span(node),
            }))?;
        }
        Ok(())
    }
}

pub(super) fn extension(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == "extension")
}
