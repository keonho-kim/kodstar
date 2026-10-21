// Purpose: Scala val/var·생성자 필드·지역 생성/반환 값의 바인딩 추출.
// Pattern: Simple Module.
// Usage: 선언·변수 추출기가 Scala 값 노드와 소유 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/scala/mod.rs, crates/kodstar-core/src/application/analysis/resolve/field_types.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

fn field(node: Node<'_>) -> bool {
    if node.kind() == "class_parameter" {
        let mut cursor = node.walk();
        if !node
            .children(&mut cursor)
            .any(|n| matches!(n.kind(), "val" | "var"))
        {
            return false;
        }
    } else if !matches!(
        node.kind(),
        "val_definition" | "var_definition" | "val_declaration" | "var_declaration"
    ) {
        return false;
    }
    let mut parent = node.parent();
    while let Some(current) = parent {
        if matches!(
            current.kind(),
            "function_definition" | "function_declaration" | "lambda_expression"
        ) {
            return false;
        }
        if matches!(
            current.kind(),
            "class_definition" | "trait_definition" | "object_definition" | "given_definition"
        ) {
            return true;
        }
        parent = current.parent();
    }
    false
}

fn name(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("pattern")
        .or_else(|| node.child_by_field_name("name"))
        .filter(|n| n.kind() == "identifier")
}

fn value_type(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("type").or_else(|| {
        node.child_by_field_name("value")
            .filter(|n| n.kind() == "instance_expression")
            .and_then(|n| {
                child_kind(
                    n,
                    &["type_identifier", "stable_type_identifier", "generic_type"],
                )
            })
    })
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn scala_value_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language != Language::Scala || !field(node) {
            return Ok(None);
        }
        name(node)
            .map(|n| Ok((self.bounded_text(text(n, self.source))?, SymbolKind::Field)))
            .transpose()
    }

    pub(in crate::infrastructure::parse) fn scala_property(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if self.language != Language::Scala {
            return Ok(());
        }
        if matches!(
            node.kind(),
            "class_definition" | "trait_definition" | "enum_definition"
        ) {
            self.scala_parameters(node, c)?;
        }
        if !field(node) {
            return Ok(());
        }
        let (Some(owner), Some(name)) = (c.type_owner, name(node)) else {
            return Ok(());
        };
        let annotation = value_type(node).map(|n| self.type_text(n)).transpose()?;
        let producer = node
            .child_by_field_name("value")
            .filter(|n| n.kind() == "call_expression")
            .map(|n| self.call_text(n, self.callee(n).unwrap_or(n)))
            .transpose()?;
        let kind = if annotation.is_some() {
            "field"
        } else {
            "field_factory"
        };
        let spelling = self.bounded_text(text(name, self.source))?;
        (self.emit)(Fact::Binding(Binding {
            scope: owner,
            name: spelling,
            target: Some(c.source),
            module: None,
            imported_name: producer,
            type_name: annotation,
            kind,
            span: span(name),
        }))
    }

    pub(in crate::infrastructure::parse) fn scala_binding(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<bool> {
        if self.language != Language::Scala
            || !matches!(
                node.kind(),
                "val_definition" | "var_definition" | "val_declaration" | "var_declaration"
            )
        {
            return Ok(false);
        }
        if field(node) {
            return Ok(true);
        }
        let Some(name) = name(node) else {
            return Ok(true);
        };
        let annotation = value_type(node).map(|n| self.type_text(n)).transpose()?;
        let value = node.child_by_field_name("value");
        let producer = value
            .filter(|n| n.kind() == "call_expression")
            .map(|n| self.call_text(n, self.callee(n).unwrap_or(n)))
            .transpose()?;
        let alias = value
            .filter(|n| matches!(n.kind(), "identifier" | "field_expression"))
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
