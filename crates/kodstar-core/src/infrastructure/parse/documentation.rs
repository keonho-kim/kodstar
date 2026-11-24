// Purpose: 실제 주석 노드에서 명시적인 ADR·RFC Markdown 참조를 수집.
// Pattern: Tree Visitor.
// Usage: 참조 추출기가 현재 구문 노드와 소유 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/references.rs, crates/kodstar-core/src/domain/resolve/documentation.rs.

use std::path::Path;

use crate::domain::{diagnostics::Result, relations::RelationKind, resolve::documentation::PREFIX};
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn documentation_references(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if !matches!(
            node.kind(),
            "comment" | "line_comment" | "block_comment" | "doc_comment" | "documentation_comment"
        ) {
            return Ok(());
        }
        let start = node.start_byte();
        let line_start = start.saturating_sub(node.start_position().column);
        let alone = self.source[line_start..start]
            .iter()
            .all(u8::is_ascii_whitespace);
        let placement = if alone { "leading" } else { "inside" };
        for line in text(node, self.source).lines() {
            self.cancel.check()?;
            let line =
                line.trim_start_matches(|c: char| c.is_whitespace() || "/#*;-!%".contains(c));
            let Some((kind, rest)) = ["ADR:", "RFC:", "@adr ", "@rfc "]
                .into_iter()
                .find_map(|marker| line.strip_prefix(marker).map(|rest| (marker, rest)))
            else {
                continue;
            };
            let kind = if matches!(kind, "ADR:" | "@adr ") {
                "adr"
            } else {
                "rfc"
            };
            let rest = rest.trim();
            let value = if let Some(quoted) = rest.strip_prefix('`') {
                quoted.split_once('`').map(|(target, _)| target)
            } else {
                rest.split_whitespace()
                    .next()
                    .map(|value| value.trim_end_matches([',', ';', ')', '>', '*', '/']))
            };
            let Some(value) = value.filter(|value| value.len() <= 512 && !value.contains('\0'))
            else {
                continue;
            };
            let path = value.split_once('#').map_or(value, |(path, _)| path);
            let markdown = Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("md")
                        || extension.eq_ignore_ascii_case("markdown")
                });
            if markdown {
                self.usage(
                    context,
                    format!("{PREFIX}{placement}:{kind}:{value}"),
                    RelationKind::References,
                    span(node),
                )?;
            }
        }
        Ok(())
    }
}
