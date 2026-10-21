// Purpose: Rust 생성자·enum 값 공간과 명시 macro_export scope 사실 추출.
// Pattern: Simple Module.
// Usage: 타입·enum case 정의 생성 직후 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs, crates/kodstar-core/src/infrastructure/parse/members.rs.

pub(super) mod imports;

pub(super) mod declarations;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn rust_constructor(
        &mut self,
        node: Node<'_>,
        id: u32,
        scope: u32,
    ) -> Result<()> {
        if self.language != Language::Rust || !matches!(node.kind(), "struct_item" | "enum_variant")
        {
            return Ok(());
        }
        let tuple = node
            .child_by_field_name("body")
            .is_some_and(|n| n.kind() == "ordered_field_declaration_list");
        if node.child_by_field_name("body").is_some() && !tuple {
            return Ok(());
        }
        let Some(name) = node.child_by_field_name("name") else {
            return Ok(());
        };
        let name = self.bounded_text(text(name, self.source))?;
        (self.emit)(Fact::Binding(Binding {
            scope,
            name,
            target: Some(id),
            module: None,
            imported_name: None,
            type_name: None,
            kind: if tuple {
                "rust_constructor"
            } else {
                "value_constructor"
            },
            span: span(node),
        }))
    }
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn rust_macro_export(
        &mut self,
        node: Node<'_>,
        id: u32,
    ) -> Result<()> {
        if self.language != Language::Rust || node.kind() != "macro_definition" {
            return Ok(());
        }
        let mut exported = false;
        let mut conditional = false;
        let mut previous = node.prev_named_sibling();
        while let Some(attribute) = previous.filter(|n| n.kind() == "attribute_item") {
            let value = text(attribute, self.source);
            if let Some(tail) = value.strip_prefix("#[macro_export") {
                exported |= tail.starts_with(['(', ']', ' ', '\n', '\t']);
            }
            conditional |= value.starts_with("#[cfg(") || value.starts_with("#[cfg_attr(");
            previous = attribute.prev_named_sibling();
        }
        if exported {
            (self.emit)(Fact::Binding(Binding {
                scope: 0,
                name: "@rust-macro-export".into(),
                target: Some(id),
                module: Some("crate".into()),
                imported_name: None,
                type_name: conditional.then(|| "conditional".into()),
                kind: "rust_macro_export",
                span: span(node),
            }))?;
        }
        Ok(())
    }
}
