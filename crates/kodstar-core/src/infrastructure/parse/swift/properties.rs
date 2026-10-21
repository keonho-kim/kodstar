// Purpose: Swift 지역·필드·factory와 whitelist wrapper의 구문 바인딩.
// Pattern: Simple Module.
// Usage: 선언·변수 추출기가 property AST와 소유 scope를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs, crates/kodstar-core/src/application/analysis/resolve/swift/properties.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

fn property_name(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("name").and_then(|n| {
        n.child_by_field_name("bound_identifier")
            .or_else(|| child_kind(n, &["simple_identifier"]))
    })
}

fn field(node: Node<'_>) -> bool {
    let mut parent = node.parent();
    while let Some(node) = parent {
        if matches!(
            node.kind(),
            "function_declaration"
                | "init_declaration"
                | "deinit_declaration"
                | "subscript_declaration"
        ) {
            return false;
        }
        if matches!(node.kind(), "class_declaration" | "protocol_declaration") {
            return true;
        }
        parent = node.parent();
    }
    false
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn swift_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language == Language::Swift && node.kind() == "property_declaration" && field(node)
        {
            return property_name(node)
                .map(|n| Ok((self.bounded_text(text(n, self.source))?, SymbolKind::Field)))
                .transpose();
        }
        Ok(None)
    }

    fn swift_environment<'a>(&self, node: Node<'a>) -> Option<Node<'a>> {
        let modifiers = child_kind(node, &["modifiers"])?;
        let mut cursor = modifiers.walk();
        for attribute in modifiers
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "attribute")
        {
            let head = child_kind(attribute, &["user_type"])?;
            if text(head, self.source) != "Environment" {
                continue;
            }
            let Some(argument) = child_kind(attribute, &["navigation_expression"]) else {
                continue;
            };
            let (Some(receiver), Some(suffix)) = (
                argument.child_by_field_name("target"),
                argument.child_by_field_name("suffix"),
            ) else {
                continue;
            };
            if receiver.kind() == "simple_identifier"
                && text(suffix, self.source) == ".self"
                && text(receiver, self.source)
                    .chars()
                    .next()
                    .is_some_and(char::is_uppercase)
            {
                return Some(receiver);
            }
        }
        None
    }

    pub(in crate::infrastructure::parse) fn swift_property(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if self.language != Language::Swift || node.kind() != "property_declaration" || !field(node)
        {
            return Ok(());
        }
        let (Some(owner), Some(name)) = (c.type_owner, property_name(node)) else {
            return Ok(());
        };
        let annotation = child_kind(node, &["type_annotation"])
            .map(|n| self.type_text(n))
            .transpose()?;
        let environment = annotation
            .is_none()
            .then(|| self.swift_environment(node))
            .flatten();
        let annotation = annotation.or(environment.map(|n| self.type_text(n)).transpose()?);
        let producer = node
            .child_by_field_name("value")
            .filter(|n| n.kind() == "call_expression")
            .map(|n| self.call_text(n, self.callee(n).unwrap_or(n)))
            .transpose()?;
        let spelling = self.bounded_text(text(name, self.source))?;
        (self.emit)(Fact::Binding(Binding {
            scope: owner,
            name: spelling,
            target: Some(c.source),
            module: None,
            imported_name: if environment.is_some() {
                annotation.clone()
            } else {
                producer
            },
            type_name: if environment.is_some() {
                None
            } else {
                annotation
            },
            kind: if child_kind(node, &["type_annotation"]).is_some() {
                "field"
            } else if environment.is_some() {
                "swift_environment"
            } else {
                "field_factory"
            },
            span: span(name),
        }))?;
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn swift_binding(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<bool> {
        if self.language != Language::Swift || node.kind() != "property_declaration" {
            return Ok(false);
        }
        if field(node) {
            return Ok(true);
        }
        let Some(name) = property_name(node) else {
            return Ok(true);
        };
        let annotation = child_kind(node, &["type_annotation"])
            .map(|n| self.type_text(n))
            .transpose()?;
        let value = node.child_by_field_name("value");
        let producer = value
            .filter(|n| n.kind() == "call_expression")
            .map(|n| self.call_text(n, self.callee(n).unwrap_or(n)))
            .transpose()?;
        let alias = value
            .filter(|n| matches!(n.kind(), "navigation_expression" | "simple_identifier"))
            .map(|n| self.reference_text(n))
            .transpose()?;
        let kind = if annotation.is_some() {
            "local"
        } else if producer.is_some() {
            "call_result"
        } else if alias.is_some() {
            "alias"
        } else {
            "local"
        };
        let spelling = self.bounded_text(text(name, self.source))?;
        (self.emit)(Fact::Binding(Binding {
            scope: c.scope,
            name: spelling,
            target: None,
            module: None,
            imported_name: producer.or(alias),
            type_name: annotation,
            kind,
            span: span(name),
        }))?;
        Ok(true)
    }
}
