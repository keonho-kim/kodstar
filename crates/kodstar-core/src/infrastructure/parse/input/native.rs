// Purpose: native 매크로의 선언 수식어·독립 구문 경계를 원본 위치를 유지해 표시.
// Pattern: Source Adapter.
// Usage: parser가 구문 실패나 전역 매크로 함수 형태를 발견하면 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/input/native_tokens.rs, crates/kodstar-core/src/infrastructure/parse/mod.rs.

use super::native_headers::{SyntaxContext, context as macro_context};
use super::native_ranges::contains;
use super::native_tokens::{
    block_macro, call_end, continues_expression, control_keyword, declaration_after, directive_end,
    macro_name, parameter_tail, previous_word, standalone, statement_boundary, tail, token_end,
};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::ops::Range;
use tree_sitter::Tree;

pub(super) const MODIFIER: u8 = 0x1f;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(in crate::infrastructure::parse) struct Marker {
    pub offset: usize,
    pub byte: u8,
}

pub(in crate::infrastructure::parse) fn prepare(
    tree: &Tree,
    source: &[u8],
    original: &[u8],
    limit: usize,
    cancel: &Cancellation,
) -> Result<Vec<Marker>> {
    let super::native_evidence::Evidence {
        directives,
        errors,
        types,
        protected,
        explicit,
        statements,
        mut structural,
        mut functions,
        mut macros,
        modifiers,
        suffixes,
        mut qualifiers,
    } = super::native_evidence::collect(tree, source, limit, cancel)?;
    let eligible = |at| contains(&errors, at) && !contains(&protected, at);
    functions.retain(|(name, kind)| {
        let known = |target: &Range<usize>| {
            macros.iter().any(|range| {
                source.get(range.start) == Some(&b'@')
                    && original[range.clone()] == source[target.clone()]
            })
        };
        let declared_type = types
            .iter()
            .any(|range| original[range.clone()] == original[kind.clone()]);
        let macro_call = known(name) && !declared_type;
        let shadow = !declared_type
            && standalone(source, kind.start)
            && macro_name(&source[kind.clone()])
            && (known(kind) || macro_call);
        if shadow {
            structural.push(kind.start);
        }
        !shadow && !macro_call
    });
    let mut output: Vec<Marker> = modifiers
        .into_iter()
        .map(|offset| modifier_marker(source, offset))
        .chain(
            structural
                .iter()
                .copied()
                .map(|offset| Marker { offset, byte: b'@' }),
        )
        .chain(suffixes.iter().copied().map(|offset| Marker {
            offset,
            byte: MODIFIER,
        }))
        .collect();
    let selected = explicit
        .into_iter()
        .map(|at| (at, modifier_marker(source, at).byte, false))
        .chain(statements.into_iter().map(|at| (at, b'@', true)));
    for (at, mut byte, statement) in selected {
        if macro_context(tree, at) == SyntaxContext::Type {
            byte = MODIFIER;
        }
        let end = source[at..]
            .iter()
            .position(|b| !b.is_ascii_alphanumeric() && *b != b'_')
            .map_or(source.len(), |n| at + n);
        if control_keyword(&source[at..end]) {
            continue;
        }
        if statement
            && !macro_name(&source[at..end])
            && !source[end..].trim_ascii_start().starts_with(b"(")
        {
            continue;
        }
        let known = macros
            .iter()
            .any(|range| original[range.clone()] == source[at..end]);
        if statement && !known && !macro_name(&source[at..end]) && !statement_boundary(source, at) {
            continue;
        }

        if block_macro(source, at, end, cancel)?
            || macro_context(tree, at) != SyntaxContext::Type && tail(source, end, true, cancel)?
        {
            byte = b'@';
        }
        if byte == b'@' && !known && !statement && !macro_name(&source[at..end]) {
            continue;
        }
        if eligible(at)
            && !functions
                .iter()
                .any(|(range, _)| source[range.clone()] == source[at..end])
        {
            output.push(Marker { offset: at, byte });
            if byte == MODIFIER {
                qualifiers.push(at..end);
            } else {
                macros.push(at..end);
            }
        }
    }
    let mut at = 0;
    while at < source.len() {
        cancel.check()?;
        let start = at;
        if let Some(end) = directive_end(source, at) {
            at = end;
            continue;
        }
        if matches!(source[at], b'@' | MODIFIER) {
            at += 1;
            while source
                .get(at)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                at += 1;
            }
            continue;
        }
        if output.len().saturating_mul(16) > limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "native macro marker budget exceeded",
            ));
        }
        if let Some(end) = token_end(source, at) {
            at = end;
            continue;
        }
        if source[at].is_ascii_alphabetic() || source[at] == b'_' {
            at += 1;
            while source
                .get(at)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                at += 1;
            }
            let known_macro = macros
                .iter()
                .any(|range| original[range.clone()] == source[start..at]);
            let conditional_modifier = previous_word(source, start) == b"if";
            if conditional_modifier && (known_macro || macro_name(&source[start..at])) {
                output.retain(|marker| marker.offset != start);
                structural.retain(|offset| *offset != start);
                output.push(Marker {
                    offset: start,
                    byte: MODIFIER,
                });
                continue;
            }
            if (known_macro || macro_name(&source[start..at]))
                && continues_expression(source, at, cancel)?
            {
                output.retain(|marker| marker.offset != start || marker.byte != b'@');
                structural.retain(|offset| *offset != start);
            }
            let line = memchr::memrchr(b'\n', &source[..start]).map_or(0, |n| n + 1);
            let prefix = source[line..start].iter().all(u8::is_ascii_whitespace);
            let previous = source[..start].trim_ascii_end().last().copied();
            if known_macro
                && prefix
                && !contains(&protected, start)
                && !matches!(previous, Some(b')' | b',' | b'(' | b'<'))
                && tail(source, at, true, cancel)?
            {
                structural.push(start);
                output.push(Marker {
                    offset: start,
                    byte: b'@',
                });
                continue;
            }
            if contains(&protected, start) && previous_word(source, start) != b"if" {
                continue;
            }
            if qualifiers
                .iter()
                .any(|range| original[range.clone()] == source[start..at])
            {
                let mut marker = modifier_marker(source, start);
                if macro_context(tree, start) == SyntaxContext::Type {
                    marker.byte = MODIFIER;
                }
                let known = macros
                    .iter()
                    .any(|range| original[range.clone()] == source[start..at]);
                if block_macro(source, start, at, cancel)?
                    || known
                        && macro_context(tree, start) != SyntaxContext::Type
                        && tail(
                            source,
                            at,
                            macro_context(tree, start) == SyntaxContext::Declaration,
                            cancel,
                        )?
                {
                    marker.byte = b'@';
                }
                output.push(marker);
                continue;
            }
            let function = functions
                .iter()
                .any(|(range, _)| source[range.clone()] == source[start..at]);
            if function
                || contains(&protected, start) && !conditional_modifier
                || !(known_macro
                    || macro_name(&source[start..at]) && (eligible(start) || conditional_modifier))
            {
                continue;
            }
            let context = macro_context(tree, start);
            let dependent_modifier =
                matches!(previous_word(source, start), b"typename" | b"typedef")
                    && source[at..].trim_ascii_start().starts_with(b"(");
            let type_modifier = context == SyntaxContext::Type || dependent_modifier;
            let type_call = type_modifier
                && source[at..].trim_ascii_start().starts_with(b"(")
                && call_end(source, at, cancel)?.is_some_and(|end| {
                    source[end..]
                        .trim_ascii_start()
                        .first()
                        .is_some_and(|b| b"*&>,)=;".contains(b))
                });
            if !prefix
                && !conditional_modifier
                && !type_modifier
                && !(known_macro && previous == Some(b')'))
                && !matches!(previous, Some(b'{' | b'}' | b';'))
            {
                continue;
            }
            let semicolon = context != SyntaxContext::Type;
            if conditional_modifier
                || dependent_modifier
                || type_call
                || type_modifier && parameter_tail(source, at)
                || declaration_after(source, at)
                || tail(source, at, semicolon, cancel)?
            {
                output.push(Marker {
                    offset: start,
                    byte: if !block_macro(source, start, at, cancel)?
                        && (conditional_modifier || type_modifier)
                    {
                        MODIFIER
                    } else {
                        b'@'
                    },
                });
            }
        } else {
            at += 1;
        }
    }
    output.sort_unstable();
    output.dedup_by_key(|marker| marker.offset);
    for offset in structural {
        if let Ok(index) = output.binary_search_by_key(&offset, |marker| marker.offset) {
            output[index].byte = b'@';
        }
    }
    for offset in suffixes {
        if let Ok(index) = output.binary_search_by_key(&offset, |marker| marker.offset) {
            output[index].byte = MODIFIER;
        }
    }
    output.retain(|marker| {
        if contains(&directives, marker.offset) {
            return false;
        }
        let end = original[marker.offset..]
            .iter()
            .position(|b| !b.is_ascii_alphanumeric() && *b != b'_')
            .map_or(original.len(), |n| marker.offset + n);
        !types
            .iter()
            .any(|range| original[range.clone()] == original[marker.offset..end])
    });
    if output.iter().any(|marker| marker.byte == b'@') {
        output.retain(|marker| marker.byte == b'@');
    }
    Ok(output)
}

fn modifier_marker(source: &[u8], offset: usize) -> Marker {
    let end = source[offset..]
        .iter()
        .position(|b| !b.is_ascii_alphanumeric() && *b != b'_')
        .map_or(source.len(), |length| offset + length);
    Marker {
        offset,
        byte: if standalone(source, offset) || declaration_after(source, end) {
            b'@'
        } else {
            MODIFIER
        },
    }
}
