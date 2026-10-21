// Purpose: PHP 상수·trait 사용·반환 타입과 생성 결과 binding의 구문 사실 추출.
// Pattern: Simple Module.
// Usage: 선언·참조·변수 추출기가 PHP 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs, crates/kodstar-core/src/application/analysis/resolve/php/members.rs.

pub(super) mod imports;
pub(super) mod traits;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn php_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language == Language::Php
            && node.kind() == "const_element"
            && let Some(name) = node.named_child(0)
        {
            return Ok(Some((
                self.bounded_text(text(name, self.source))?,
                SymbolKind::Field,
            )));
        }
        Ok(None)
    }

    pub(in crate::infrastructure::parse) fn php_callable(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if self.language != Language::Php {
            return Ok(());
        }
        if node.kind() == "method_declaration" {
            let visibility = child_kind(node, &["visibility_modifier"])
                .map_or("public", |n| text(n, self.source));
            let visibility = self.bounded_text(visibility)?;
            (self.emit)(Fact::Binding(Binding {
                scope: c.scope,
                name: "@visibility".into(),
                target: Some(c.source),
                module: Some(visibility),
                imported_name: None,
                type_name: None,
                kind: "php_visibility",
                span: span(node),
            }))?;
        }
        if let Some(ty) = node.child_by_field_name("return_type") {
            let annotation = self.type_text(ty)?;
            self.return_annotation(c, ty, annotation)?;
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn php_binding(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<bool> {
        if self.language != Language::Php || node.kind() != "assignment_expression" {
            return Ok(false);
        }
        let (Some(name), Some(value)) = (
            node.child_by_field_name("left")
                .filter(|n| n.kind() == "variable_name"),
            node.child_by_field_name("right"),
        ) else {
            return Ok(true);
        };
        let producer = if matches!(
            value.kind(),
            "object_creation_expression"
                | "function_call_expression"
                | "member_call_expression"
                | "scoped_call_expression"
                | "nullsafe_member_call_expression"
        ) {
            let callee = value
                .child_by_field_name("function")
                .or_else(|| value.named_child(0))
                .unwrap_or(value);
            Some(self.call_text(value, callee)?)
        } else {
            None
        };
        let name_text = self.bounded_text(text(name, self.source).trim_start_matches('$'))?;
        (self.emit)(Fact::Binding(Binding {
            scope: c.function_scope,
            name: name_text,
            target: None,
            module: None,
            imported_name: producer.clone(),
            type_name: None,
            kind: if producer.is_some() {
                "call_result"
            } else {
                "local"
            },
            span: span(name),
        }))?;
        Ok(true)
    }

    pub(in crate::infrastructure::parse) fn php_facts(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if self.language != Language::Php {
            return Ok(());
        }
        if node.kind() == "use_declaration" {
            let mut cursor = node.walk();
            for name in node
                .named_children(&mut cursor)
                .filter(|n| matches!(n.kind(), "name" | "qualified_name" | "relative_name"))
            {
                self.usage(
                    c,
                    self.reference_text(name)?,
                    RelationKind::MixesIn,
                    span(name),
                )?;
            }
        }
        self.php_trait_rule(node, c)?;
        if node.kind() == "class_constant_access_expression" {
            if node
                .parent()
                .is_some_and(|p| matches!(p.kind(), "use_as_clause" | "use_instead_of_clause"))
            {
                return Ok(());
            }
            let mut cursor = node.walk();
            let mut names = node.named_children(&mut cursor);
            if let (Some(owner), Some(name)) = (names.next(), names.next()) {
                if text(name, self.source).eq_ignore_ascii_case("class") {
                    self.usage(
                        c,
                        self.reference_text(owner)?,
                        RelationKind::UsesType,
                        span(owner),
                    )?;
                } else {
                    let mut expression = String::new();
                    self.append_signature(
                        &mut expression,
                        &[
                            &self.reference_text(owner)?,
                            "::",
                            &self.reference_text(name)?,
                        ],
                    )?;
                    self.usage(c, expression, RelationKind::References, span(node))?;
                }
            }
        }
        if matches!(node.kind(), "name" | "qualified_name" | "relative_name")
            && node.parent().is_some_and(|p| {
                matches!(
                    p.kind(),
                    "argument"
                        | "return_statement"
                        | "echo_statement"
                        | "binary_expression"
                        | "conditional_expression"
                        | "array_element_initializer"
                )
            })
        {
            self.usage(
                c,
                self.reference_text(node)?,
                RelationKind::References,
                span(node),
            )?;
        }
        if node.kind() == "attribute"
            && let Some(name) = child_kind(node, &["name", "qualified_name", "relative_name"])
        {
            self.usage(
                c,
                self.reference_text(name)?,
                RelationKind::UsesType,
                span(name),
            )?;
        }
        Ok(())
    }
}
