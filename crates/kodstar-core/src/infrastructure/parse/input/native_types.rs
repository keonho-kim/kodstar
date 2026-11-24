// Purpose: 매크로 보정에서 보호할 실제 타입 선언 이름의 근거 수집.
// Pattern: Syntax Evidence.
// Usage: native evidence가 어휘·AST 선언을 모으고 marker 선택기가 소비.
// Related: crates/kodstar-core/src/infrastructure/parse/input/native_evidence.rs, crates/kodstar-core/src/infrastructure/parse/input/native_tokens.rs.

use super::native_tokens::{call_end, directive_end, macro_name, token_end};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::ops::Range;
use tree_sitter::Node;

pub(super) fn lexical(
    source: &[u8],
    limit: usize,
    cancel: &Cancellation,
) -> Result<Vec<Range<usize>>> {
    let mut types = Vec::new();
    let mut at = 0;
    while at < source.len() {
        cancel.check()?;
        if let Some(end) = directive_end(source, at).or_else(|| token_end(source, at)) {
            at = end;
            continue;
        }
        if !source[at].is_ascii_alphabetic() && source[at] != b'_' {
            at += 1;
            continue;
        }
        let start = at;
        at = word_end(source, at);
        let keyword = &source[start..at];
        if !matches!(
            keyword,
            b"typename" | b"class" | b"struct" | b"enum" | b"using"
        ) {
            continue;
        }
        let mut name = at;
        while source.get(name).is_some_and(u8::is_ascii_whitespace) {
            name += 1;
        }
        if source.get(name..name.saturating_add(3)) == Some(b"...") {
            name += 3;
        }
        while source.get(name).is_some_and(u8::is_ascii_whitespace) {
            name += 1;
        }
        let mut end = word_end(source, name);
        if matches!(keyword, b"class" | b"struct" | b"enum") {
            while macro_name(&source[name..end]) {
                let Some(after) = call_end(source, end, cancel)? else {
                    break;
                };
                let next = source.len() - source[after..].trim_ascii_start().len();
                let next_end = word_end(source, next);
                if next_end == next || &source[next..next_end] == b"final" {
                    break;
                }
                let following = source[next_end..].trim_ascii_start();
                if !source[end..].trim_ascii_start().starts_with(b"(")
                    && !following.first().is_some_and(|byte| b"{:<".contains(byte))
                    && !following.starts_with(b"final")
                {
                    break;
                }
                name = next;
                end = next_end;
            }
        }
        if end == name {
            continue;
        }
        let rest = source[end..].trim_ascii_start();
        if rest.starts_with(b"(") || keyword == b"using" && !rest.starts_with(b"=") {
            continue;
        }
        if rest
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
            && !rest.starts_with(b"final")
        {
            continue;
        }
        remember(&mut types, name..end, source);
        if types.len().saturating_mul(64) > limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "native type names exceeded their byte budget",
            ));
        }
    }
    Ok(types)
}

pub(super) fn declared(node: Node<'_>, source: &[u8]) -> Option<Range<usize>> {
    if matches!(
        node.kind(),
        "parameter_declaration" | "optional_parameter_declaration"
    ) && !node.has_error()
        && node.child_by_field_name("declarator").is_some()
        && let Some(name) = super::super::syntax::name_node(node)
        && name.kind() == "identifier"
        && macro_name(&source[name.byte_range()])
    {
        return Some(name.byte_range());
    }
    if node.kind() == "type_definition" && node.has_error() {
        return None;
    }
    if !matches!(
        node.kind(),
        "type_parameter_declaration"
            | "optional_type_parameter_declaration"
            | "variadic_type_parameter_declaration"
            | "class_specifier"
            | "struct_specifier"
            | "enum_specifier"
            | "alias_declaration"
            | "type_definition"
    ) {
        return None;
    }
    if super::native_headers::misplaced_declaration_macro(node, source).is_some() {
        return None;
    }
    let name = super::super::syntax::name_node(node)?;
    if matches!(node.kind(), "class_specifier" | "struct_specifier")
        && source[name.end_byte()..]
            .trim_ascii_start()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    Some(name.byte_range())
}

pub(super) fn remember(types: &mut Vec<Range<usize>>, range: Range<usize>, source: &[u8]) {
    if !types
        .iter()
        .any(|known| source[known.clone()] == source[range.clone()])
    {
        types.push(range);
    }
}

fn word_end(source: &[u8], mut at: usize) -> usize {
    while source
        .get(at)
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        at += 1;
    }
    at
}
