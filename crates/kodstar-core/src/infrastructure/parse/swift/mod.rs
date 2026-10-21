// Purpose: Swift 반환·매개변수·서명의 명시적 타입 사실 추출.
// Pattern: Simple Module.
// Usage: 선언·변수 추출기가 Swift AST와 현재 scope를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs, crates/kodstar-core/src/application/analysis/resolve/field_types.rs.

pub(super) mod packages;
pub(super) mod properties;

pub(super) mod imports;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

fn parameter_name(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.children_by_field_name("name", &mut cursor)
        .find(|n| n.kind() == "simple_identifier")
}

fn type_node(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.children_by_field_name("name", &mut cursor).find(|n| {
        matches!(
            n.kind(),
            "user_type"
                | "array_type"
                | "dictionary_type"
                | "tuple_type"
                | "function_type"
                | "optional_type"
        )
    })
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn swift_exported(&self, node: Node<'_>) -> bool {
        let Some(modifiers) = child_kind(node, &["modifiers"]) else {
            return false;
        };
        let mut cursor = modifiers.walk();
        modifiers
            .named_children(&mut cursor)
            .any(|n| matches!(text(n, self.source), "public" | "open"))
    }
    pub(in crate::infrastructure::parse) fn swift_callable(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if self.language != Language::Swift {
            return Ok(());
        }
        if let Some(ty) = type_node(node) {
            self.return_annotation(c, ty, self.type_text(ty)?)?;
        }
        let mut cursor = node.walk();
        for parameter in node
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "parameter")
        {
            if let Some(name) = parameter_name(parameter) {
                let spelling = self.bounded_text(text(name, self.source))?;
                let annotation = type_node(parameter)
                    .map(|n| self.type_text(n))
                    .transpose()?;
                (self.emit)(Fact::Binding(Binding {
                    scope: c.scope,
                    name: spelling,
                    target: None,
                    module: None,
                    imported_name: None,
                    type_name: annotation,
                    kind: "parameter",
                    span: span(name),
                }))?;
            }
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn swift_signature(
        &self,
        node: Node<'_>,
    ) -> Result<String> {
        let mut result = String::from("(");
        let mut cursor = node.walk();
        for parameter in node
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "parameter")
        {
            let name = parameter_name(parameter)
                .map(|n| self.parameter_text(n))
                .transpose()?
                .unwrap_or_default();
            let ty = type_node(parameter)
                .map(|n| self.type_text(n))
                .transpose()?
                .unwrap_or_default();
            let label = parameter
                .child_by_field_name("external_name")
                .map(|n| self.parameter_text(n))
                .transpose()?
                .unwrap_or_default();
            self.append_signature(&mut result, &[&label, "/", &name, ":", &ty, ","])?;
        }
        self.append_signature(&mut result, &[")"])?;
        if let Some(ty) = type_node(node) {
            self.append_signature(&mut result, &[&self.type_text(ty)?])?;
        }
        Ok(result)
    }
}
