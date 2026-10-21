// Purpose: Ruby 정적 require·load의 바인딩 추출.
// Pattern: Simple Module.
// Usage: import dispatcher가 해당 언어의 호출·변수 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs, crates/kodstar-core/src/infrastructure/parse/bindings.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::{Imports, unquote};
use crate::infrastructure::parse::{span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn ruby(&mut self, node: Node<'_>) -> Result<()> {
        if node.child_by_field_name("receiver").is_some() {
            return Ok(());
        }
        let Some(method) = node.child_by_field_name("method") else {
            return Ok(());
        };
        let method = text(method, self.source);
        if !matches!(method, "require" | "require_relative" | "load") {
            return Ok(());
        }
        let Some(args) = node.child_by_field_name("arguments") else {
            return Ok(());
        };
        if args.named_child_count() != 1 {
            return Ok(());
        }
        let Some(value) = args.named_child(0).filter(|n| n.kind() == "string") else {
            return Ok(());
        };
        if value.named_child_count() != 1 {
            return Ok(());
        }
        let raw = unquote(text(value, self.source));
        let module = if method == "require_relative" {
            format!("./{raw}")
        } else {
            raw.into()
        };
        self.bind("*", &module, Some("*"), span(value))
    }
}
