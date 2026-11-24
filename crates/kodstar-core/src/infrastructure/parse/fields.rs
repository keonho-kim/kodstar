// Purpose: 명시적 필드·승격 property의 타입·소유 추출.
// Pattern: Simple Module.
// Usage: 선언 순회가 타입 문맥과 필드 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/application/analysis/resolve/field_types.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::{Binding, Definition, Fact};
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn field(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        let Some(owner) = context.type_owner else {
            return Ok(());
        };
        let (declaration, ty) = if field_variable(node) {
            let parent = node.parent().unwrap_or(node);
            let declaration = if parent.kind() == "variable_declaration" {
                parent.parent().unwrap_or(parent)
            } else {
                parent
            };
            (
                node,
                parent
                    .child_by_field_name("type")
                    .or_else(|| declaration.child_by_field_name("type")),
            )
        } else if matches!(
            node.kind(),
            "field_declaration"
                | "property_element"
                | "property_promotion_parameter"
                | "field_definition"
                | "property_signature"
        ) {
            let ty = node
                .child_by_field_name("type")
                .or_else(|| node.parent().and_then(|p| p.child_by_field_name("type")));
            (node, ty)
        } else {
            return Ok(());
        };
        let Some(ty) = ty else { return Ok(()) };
        let mut names = declaration.walk();
        let mut found = false;
        for name in declaration.children_by_field_name("name", &mut names) {
            if name.is_named() {
                self.field_definition(name, ty, owner, context)?;
                found = true;
            }
        }
        if !found
            && matches!(
                self.language,
                crate::domain::languages::Language::C | crate::domain::languages::Language::Cpp
            )
        {
            let mut cursor = declaration.walk();
            for declarator in declaration.children_by_field_name("declarator", &mut cursor) {
                if let Some(name) = super::cxx::declarators::value_name(declarator) {
                    self.field_definition(name, ty, owner, context)?;
                    found = true;
                }
            }
        }
        if !found
            && let Some(name) = declaration
                .child_by_field_name("declarator")
                .filter(|n| matches!(n.kind(), "field_identifier" | "identifier"))
        {
            self.field_definition(name, ty, owner, context)?;
        }
        if !found && self.language == crate::domain::languages::Language::Go {
            let mut base = ty;
            while let Some(inner) = base.child_by_field_name("type") {
                base = inner;
            }
            let name = base.child_by_field_name("name").unwrap_or(base);
            if matches!(name.kind(), "type_identifier" | "identifier") {
                self.field_definition(name, ty, owner, context)?;
                self.usage(
                    context,
                    self.type_text(ty)?,
                    crate::domain::relations::RelationKind::MixesIn,
                    span(ty),
                )?;
            }
        }
        if !found
            && let Some(name) = declaration
                .child_by_field_name("property")
                .filter(|n| n.kind() == "property_identifier")
        {
            self.field_definition(name, ty, owner, context)?;
        }
        Ok(())
    }

    fn field_definition(
        &mut self,
        name: Node<'_>,
        ty: Node<'_>,
        owner: u32,
        context: &Context,
    ) -> Result<()> {
        let local_id = self.allocate()?;
        let spelling = self.bounded_text(text(name, self.source).trim_start_matches('$'))?;
        let qualified_name = context
            .type_name
            .as_ref()
            .unwrap_or(&context.prefix)
            .child(&spelling, self.fact_limit)?
            .render()?;
        let fact = Fact::Definition(Definition {
            local_id,
            owner: Some(owner),
            scope: owner,
            name: spelling.clone(),
            qualified_name,
            kind: SymbolKind::Field,
            signature: String::new(),
            condition: context.condition.as_deref().map(str::to_owned),
            span: span(name),
            exported: self.language != crate::domain::languages::Language::Go
                || spelling.chars().next().is_some_and(char::is_uppercase),
        });
        if fact.owned_bytes() > self.fact_limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "field definition exceeds fact_batch_bytes",
            ));
        }
        (self.emit)(fact)?;
        let type_name = Some(self.type_text(ty)?);
        (self.emit)(Fact::Binding(Binding {
            scope: owner,
            name: spelling,
            target: Some(local_id),
            module: None,
            imported_name: None,
            type_name,
            kind: "field",
            span: span(name),
        }))
    }
}

pub(super) fn field_variable(node: Node<'_>) -> bool {
    node.kind() == "variable_declarator"
        && node.parent().is_some_and(|p| {
            p.kind() == "field_declaration"
                || p.kind() == "variable_declaration"
                    && p.parent().is_some_and(|p| p.kind() == "field_declaration")
        })
}
