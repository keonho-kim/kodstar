// Purpose: EJS server include 호출의 정적 경로 또는 동적 미해결 사실 추출.
// Pattern: Simple Module.
// Usage: server EJS program의 참조 walker가 호출 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/ejs/mod.rs, crates/kodstar-core/src/application/analysis/resolve/ejs.rs.

use crate::domain::diagnostics::Result;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn ejs_include(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if !self.ejs_server
            || node.kind() != "call_expression"
            || !node
                .child_by_field_name("function")
                .is_some_and(|n| n.kind() == "identifier" && text(n, self.source) == "include")
        {
            return Ok(());
        }
        let argument = node
            .child_by_field_name("arguments")
            .and_then(|n| n.named_child(0));
        let mut position = span(argument.unwrap_or(node));
        let mut expression = "@ejs-include:dynamic".to_owned();
        if let Some(argument) =
            argument.filter(|n| matches!(n.kind(), "string" | "template_string"))
        {
            let raw = text(argument, self.source);
            if raw.len() >= 2
                && !raw.contains('\\')
                && !(argument.kind() == "template_string" && raw.contains("${"))
            {
                let value = &raw[1..raw.len() - 1];
                expression.clear();
                self.append_signature(&mut expression, &["@ejs-include:literal:", value])?;
                position.start_byte += 1;
                position.end_byte -= 1;
                position.column += 1;
                position.end_column = position.end_column.saturating_sub(1);
            }
        }
        self.usage(context, expression, RelationKind::TemplateInclude, position)
    }
}
