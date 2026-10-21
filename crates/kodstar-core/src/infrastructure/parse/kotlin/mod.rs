// Purpose: Kotlin 반환 annotation과 property 지역 값·extension receiver binding 추출.
// Pattern: Simple Module.
// Usage: 함수 선언·변수 추출기가 Kotlin AST와 현재 scope를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/return_values.rs, crates/kodstar-core/src/infrastructure/parse/bindings.rs.

pub(super) mod imports;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

pub(in crate::infrastructure::parse) fn return_type(node: Node<'_>) -> Option<Node<'_>> {
    let parameters = node
        .child_by_field_name("parameters")
        .or_else(|| child_kind(node, &["function_value_parameters"]))?;
    let mut next = parameters.next_named_sibling();
    while let Some(node) = next {
        if matches!(
            node.kind(),
            "user_type" | "nullable_type" | "type_reference" | "function_type"
        ) {
            return Some(node);
        }
        if node.kind() == "function_body" {
            break;
        }
        next = node.next_named_sibling();
    }
    None
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn kotlin_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        Ok(
            (self.language == Language::Kotlin && node.kind() == "object_literal")
                .then(|| (format!("<object@{}>", node.start_byte()), SymbolKind::Class)),
        )
    }

    pub(in crate::infrastructure::parse) fn kotlin_declaration_binding(
        &mut self,
        node: Node<'_>,
        c: &Context,
        scope: u32,
        kind: SymbolKind,
    ) -> Result<()> {
        if self.language == Language::Kotlin
            && node.kind() == "class_declaration"
            && kind == SymbolKind::Class
            && let Some(name) = node.child_by_field_name("name")
        {
            let spelling = self.bounded_text(text(name, self.source))?;
            (self.emit)(Fact::Binding(Binding {
                scope,
                name: spelling,
                target: Some(c.source),
                module: None,
                imported_name: None,
                type_name: None,
                kind: "value_constructor",
                span: span(name),
            }))?;
        }
        if self.language != Language::Kotlin
            || !matches!(node.kind(), "object_literal" | "object_declaration")
        {
            return Ok(());
        }
        (self.emit)(Fact::Binding(Binding {
            scope: c.scope,
            name: "@kotlin-object".into(),
            target: Some(c.source),
            module: None,
            imported_name: None,
            type_name: None,
            kind: "kotlin_object_value",
            span: span(node),
        }))?;
        if node.kind() == "object_declaration"
            && let Some(name) = node.child_by_field_name("name")
        {
            let spelling = self.bounded_text(text(name, self.source))?;
            (self.emit)(Fact::Binding(Binding {
                scope,
                name: spelling,
                target: Some(c.source),
                module: None,
                imported_name: None,
                type_name: None,
                kind: "value_object",
                span: span(name),
            }))?;
        }
        if node.kind() != "object_literal" {
            return Ok(());
        }
        if let Some(name) = node
            .parent()
            .filter(|p| p.kind() == "property_declaration")
            .and_then(|p| child_kind(p, &["variable_declaration"]))
            .and_then(|n| child_kind(n, &["identifier", "simple_identifier"]))
        {
            let name_text = self.bounded_text(text(name, self.source))?;
            (self.emit)(Fact::Binding(Binding {
                scope,
                name: name_text,
                target: Some(c.source),
                module: None,
                imported_name: None,
                type_name: None,
                kind: "value_object",
                span: span(name),
            }))?;
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn kotlin_bases(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        let mut cursor = node.walk();
        for spec in node
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "delegation_specifier")
        {
            let constructor = child_kind(spec, &["constructor_invocation"]);
            let root = constructor
                .or_else(|| child_kind(spec, &["explicit_delegation"]))
                .unwrap_or(spec);
            if let Some(ty) = child_kind(root, &["user_type", "nullable_type"]) {
                self.usage(
                    c,
                    self.reference_text(ty)?,
                    if constructor.is_some() {
                        RelationKind::Inherits
                    } else {
                        RelationKind::Implements
                    },
                    span(ty),
                )?;
            }
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn kotlin_signature(
        &self,
        node: Node<'_>,
    ) -> Result<String> {
        let mut result = String::from("(");
        if let Some(parameters) = child_kind(node, &["function_value_parameters"]) {
            let mut cursor = parameters.walk();
            for parameter in parameters
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "parameter")
            {
                let name = child_kind(parameter, &["identifier", "simple_identifier"])
                    .map(|n| self.parameter_text(n))
                    .transpose()?
                    .unwrap_or_default();
                let ty = child_kind(
                    parameter,
                    &[
                        "user_type",
                        "nullable_type",
                        "type_reference",
                        "function_type",
                    ],
                )
                .map(|n| self.type_text(n))
                .transpose()?
                .unwrap_or_default();
                self.append_signature(&mut result, &[&name, ":", &ty, ","])?;
            }
        }
        self.append_signature(&mut result, &[")"])?;
        if let Some(ty) = return_type(node) {
            self.append_signature(&mut result, &[&self.type_text(ty)?])?;
        }
        Ok(result)
    }

    pub(in crate::infrastructure::parse) fn kotlin_callable(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if self.language != Language::Kotlin {
            return Ok(());
        }
        if let Some(ty) = return_type(node) {
            self.return_annotation(c, ty, self.type_text(ty)?)?;
        }
        let name = node.child_by_field_name("name");
        let receiver = child_kind(node, &["user_type", "nullable_type"])
            .filter(|n| name.is_some_and(|name| n.end_byte() < name.start_byte()));
        if let Some(receiver) = receiver {
            let annotation = self.type_text(receiver)?;
            (self.emit)(Fact::Binding(Binding {
                scope: c.scope,
                name: "this".into(),
                target: None,
                module: None,
                imported_name: None,
                type_name: Some(annotation),
                kind: "parameter",
                span: span(receiver),
            }))?;
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn kotlin_binding(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<bool> {
        if self.language != Language::Kotlin || node.kind() != "property_declaration" {
            return Ok(false);
        }
        let Some(declaration) = child_kind(node, &["variable_declaration"]) else {
            return Ok(false);
        };
        let Some(name) = child_kind(declaration, &["identifier", "simple_identifier"]) else {
            return Ok(false);
        };
        let ty = child_kind(
            declaration,
            &[
                "user_type",
                "nullable_type",
                "type_reference",
                "function_type",
            ],
        );
        let annotation = ty.map(|n| self.type_text(n)).transpose()?;
        let producer = declaration
            .next_named_sibling()
            .filter(|n| n.kind() == "call_expression")
            .map(|v| self.call_text(v, self.callee(v).unwrap_or(v)))
            .transpose()?;
        let spelling = self.bounded_text(text(name, self.source))?;
        let result = annotation.is_none() && producer.is_some();
        (self.emit)(Fact::Binding(Binding {
            scope: c.scope,
            name: spelling,
            target: None,
            module: None,
            imported_name: producer,
            type_name: annotation,
            kind: if result { "call_result" } else { "local" },
            span: span(name),
        }))?;
        Ok(true)
    }
}
