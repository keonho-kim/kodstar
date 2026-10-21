// Purpose: Python export·정적 attribute 이름에 쓰이는 비보간 문자열을 빌려 읽음.
// Pattern: Simple Module.
// Usage: __all__·getattr 추출기가 평가나 escape 추측 없이 이름을 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/python/exports.rs, crates/kodstar-core/src/infrastructure/parse/callable_values.rs.

use crate::infrastructure::parse::{child_kind, text};
use tree_sitter::Node;

pub(in crate::infrastructure::parse) fn plain_string<'source>(
    node: Node<'_>,
    source: &'source [u8],
) -> Option<&'source str> {
    if node.kind() != "string" {
        return None;
    }
    let start = child_kind(node, &["string_start"])?;
    let prefix = text(start, source).trim_end_matches(['\'', '"']);
    if !prefix.is_empty() && !prefix.eq_ignore_ascii_case("r") && !prefix.eq_ignore_ascii_case("u")
    {
        return None;
    }
    let mut cursor = node.walk();
    if !node
        .named_children(&mut cursor)
        .all(|n| matches!(n.kind(), "string_start" | "string_content" | "string_end"))
    {
        return None;
    }
    let value = child_kind(node, &["string_content"])
        .map(|n| text(n, source))
        .unwrap_or("");
    (!value.contains('\\')).then_some(value)
}
