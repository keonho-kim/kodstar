// Purpose: JSX 속성의 정적 문자열과 코드 표현 경계를 htmx adapter에 전달.
// Pattern: Syntax Adapter.
// Usage: JS/TS walker가 opening·self-closing 태그를 방문할 때 호출.
// Related: crates/kodstar-core/src/infrastructure/formats/htmx/mod.rs, crates/kodstar-core/src/infrastructure/parse/references.rs.

use crate::domain::{diagnostics::Result, languages::Language};
use crate::infrastructure::parse::{Context, Extractor};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn htmx_jsx(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if !self.htmx
            || !matches!(self.language, Language::JavaScript | Language::TypeScript)
            || !matches!(
                node.kind(),
                "jsx_opening_element" | "jsx_self_closing_element"
            )
        {
            return Ok(());
        }
        let Some(name) = node.child_by_field_name("name") else {
            return Ok(());
        };
        let language = self.language;
        crate::infrastructure::formats::htmx::attributes(
            self.source,
            name.end_byte()..node.end_byte().saturating_sub(1),
            crate::infrastructure::parse::span(name),
            crate::infrastructure::formats::htmx::Owner {
                language,
                source: context.source,
                scope: context.scope,
                disabled: context.htmx_disabled,
            },
            None,
            &mut |_, fact| (self.emit)(fact),
        )
    }
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn htmx_context(
        &self,
        node: Node<'_>,
        context: &mut Context,
    ) -> Result<()> {
        if !self.htmx
            || context.htmx_disabled
            || !matches!(node.kind(), "jsx_element" | "jsx_self_closing_element")
        {
            return Ok(());
        }
        let opening = if node.kind() == "jsx_element" {
            node.child_by_field_name("open_tag")
                .or_else(|| node.named_child(0))
        } else {
            Some(node)
        };
        if let Some(opening) = opening
            && let Some(name) = opening.child_by_field_name("name")
        {
            crate::infrastructure::formats::markup::attributes(
                self.source,
                name.end_byte()..opening.end_byte().saturating_sub(1),
                &mut |a| {
                    context.htmx_disabled |= a.name == "hx-disable" || a.name == "data-hx-disable";
                    Ok(())
                },
            )?;
        }
        Ok(())
    }
}
