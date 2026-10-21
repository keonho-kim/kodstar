// Purpose: Zig 정적 @import와 중첩 field projection의 로컬 바인딩 추출.
// Pattern: Simple Module.
// Usage: import dispatcher가 variable_declaration을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs, crates/kodstar-core/src/infrastructure/parse/context.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::infrastructure::parse::imports::{Imports, unquote};
use crate::infrastructure::parse::{NamePath, child_kind, span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn zig(&mut self, node: Node<'_>) -> Result<()> {
        let Some(name) = child_kind(node, &["identifier"]) else {
            return Ok(());
        };
        let Some(mut value) = child_kind(node, &["builtin_function", "field_expression"]) else {
            return Ok(());
        };
        let mut members = Vec::new();
        while value.kind() == "field_expression" {
            if members.len() >= 32 {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "Zig import projection exceeds 32 levels",
                ));
            }
            let (Some(object), Some(member)) = (
                value.child_by_field_name("object"),
                value.child_by_field_name("member"),
            ) else {
                return Ok(());
            };
            members.push(member);
            value = object;
        }
        let Some(builtin) = child_kind(value, &["builtin_identifier"]) else {
            return Ok(());
        };
        if text(builtin, self.source) != "@import" {
            return Ok(());
        }
        let Some(args) = child_kind(value, &["arguments"]) else {
            return Ok(());
        };
        if args.named_child_count() != 1 {
            return Ok(());
        }
        let Some(module) = args.named_child(0).filter(|n| {
            matches!(n.kind(), "string" | "string_literal") && n.named_child_count() <= 1
        }) else {
            return Ok(());
        };
        let mut path = NamePath::default();
        for member in members.into_iter().rev() {
            path = path.child(text(member, self.source), self.limit)?;
        }
        let path = path.render()?;
        self.bind(
            text(name, self.source),
            unquote(text(module, self.source)),
            (!path.is_empty()).then_some(path.as_str()),
            span(module),
        )
    }
}
