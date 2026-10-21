// Purpose: Elixir protocol·구현 컨테이너와 정적 implements 근거 추출.
// Pattern: Simple Module.
// Usage: 특수 선언 분류와 정의 완료 시 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs, crates/kodstar-core/src/infrastructure/parse/definitions.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

fn implementation_type<'t>(node: Node<'t>, source: &[u8]) -> Option<Node<'t>> {
    let args = child_kind(node, &["arguments"])?;
    let keywords = child_kind(args, &["keywords"])?;
    let mut cursor = keywords.walk();
    keywords.named_children(&mut cursor).find_map(|pair| {
        if pair
            .child_by_field_name("key")
            .is_some_and(|n| text(n, source).trim() == "for:")
        {
            pair.child_by_field_name("value")
                .filter(|n| n.kind() == "alias")
        } else {
            None
        }
    })
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn elixir_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language != Language::Elixir || node.kind() != "call" {
            return Ok(None);
        }
        if let Some(head) = crate::infrastructure::parse::elixir::head(node, self.source) {
            let keyword = node
                .child_by_field_name("target")
                .map(|n| text(n, self.source))
                .unwrap_or("");
            return Ok(Some((
                self.bounded_text(text(head.name, self.source))?,
                if keyword.contains("macro") || keyword.starts_with("defguard") {
                    SymbolKind::Macro
                } else {
                    SymbolKind::Function
                },
            )));
        }
        let keyword = node
            .child_by_field_name("target")
            .map(|n| text(n, self.source))
            .unwrap_or("");
        if !matches!(keyword, "defmodule" | "defprotocol" | "defimpl") {
            return Ok(None);
        }
        let Some(name) = child_kind(node, &["arguments"])
            .and_then(|n| n.named_child(0))
            .filter(|n| n.kind() == "alias")
        else {
            return Ok(None);
        };
        let mut name = self.bounded_text(text(name, self.source))?;
        if keyword == "defimpl" {
            let suffix = implementation_type(node, self.source).map_or_else(
                || format!("<implementation@{}>", node.start_byte()),
                |n| text(n, self.source).to_owned(),
            );
            self.append_signature(&mut name, &[".", &suffix])?;
        }
        Ok(Some((name, SymbolKind::Module)))
    }

    pub(in crate::infrastructure::parse) fn elixir_container(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Elixir
            || !node
                .child_by_field_name("target")
                .is_some_and(|n| text(n, self.source) == "defimpl")
        {
            return Ok(());
        }
        if let Some(protocol) = child_kind(node, &["arguments"])
            .and_then(|n| n.named_child(0))
            .filter(|n| n.kind() == "alias")
        {
            let mut expression = String::new();
            self.append_signature(&mut expression, &["@module:", text(protocol, self.source)])?;
            self.usage(
                context,
                expression,
                RelationKind::Implements,
                span(protocol),
            )?;
        }
        Ok(())
    }
}
