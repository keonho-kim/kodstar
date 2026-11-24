// Purpose: 전처리 조건의 어휘·논리 연산자를 정규화한 조건 식으로 변환.
// Pattern: Boundary Parser.
// Usage: conditional plan이 지시문의 조건 문자열을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/expressions.rs, crates/kodstar-core/src/infrastructure/parse/conditional/mod.rs.

use super::expressions::{Expr, Guard, and, limit, not};
use crate::domain::diagnostics::{Error, Result};
use std::borrow::Cow;
use std::sync::Arc;

fn split(text: &str, operator: &str, word: &str) -> Option<usize> {
    let mut depth = 0usize;
    let bytes = text.as_bytes();
    for (index, byte) in unquoted(text) {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0
            && text.is_char_boundary(index)
            && (text[index..].starts_with(operator)
                || text[index..]
                    .get(..word.len())
                    .is_some_and(|v| v.eq_ignore_ascii_case(word))
                    && (index == 0 || !identifier_byte(bytes[index - 1]))
                    && bytes
                        .get(index + word.len())
                        .is_none_or(|b| !identifier_byte(*b)))
        {
            return Some(index);
        }
    }
    None
}

pub(super) fn parse(text: &str, depth: usize) -> Result<Guard> {
    if depth > 128 {
        return Err(limit());
    }
    let cleaned = if depth == 0 {
        comments(text)?
    } else {
        Cow::Borrowed(text)
    };
    let text = cleaned.trim();
    for (symbol, word, conjunction) in [("||", "or", false), ("&&", "and", true)] {
        if let Some(index) = split(text, symbol, word) {
            let width = if text[index..].starts_with(symbol) {
                symbol.len()
            } else {
                word.len()
            };
            let left = parse(&text[..index], depth + 1)?;
            let right = parse(&text[index + width..], depth + 1)?;
            return Ok(if conjunction {
                and(left, right)
            } else {
                Arc::new(Expr::Or(left, right))
            });
        }
    }
    if let Some(rest) = text.strip_prefix('!') {
        return Ok(not(parse(rest, depth + 1)?));
    }
    if text.get(..3).is_some_and(|s| s.eq_ignore_ascii_case("not"))
        && text.as_bytes().get(3).is_some_and(|b| !identifier_byte(*b))
    {
        return Ok(not(parse(&text[3..], depth + 1)?));
    }
    if text.starts_with('(') && text.ends_with(')') {
        let mut level = 0isize;
        let enclosed = unquoted(&text[..text.len() - 1]).all(|(_, byte)| {
            match byte {
                b'(' => level += 1,
                b')' => level -= 1,
                _ => {}
            }
            level > 0
        });
        if enclosed {
            return parse(&text[1..text.len() - 1], depth + 1);
        }
    }
    if matches!(text, "0" | "false" | "FALSE") {
        return Ok(Arc::new(Expr::Constant(false)));
    }
    if matches!(text, "1" | "true" | "TRUE") {
        return Ok(Arc::new(Expr::Constant(true)));
    }
    let text = if text
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("defined"))
        && text
            .as_bytes()
            .get(7)
            .is_some_and(|b| *b == b'(' || b.is_ascii_whitespace())
    {
        let name = text[7..]
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();
        format!("defined({name})")
    } else {
        canonical(text)
    };
    if text.is_empty() {
        return Err(Error::input(
            "source_syntax_error",
            "empty conditional compilation expression",
            None,
        ));
    }
    Ok(Arc::new(Expr::Atom(text)))
}

fn comments(text: &str) -> Result<Cow<'_, str>> {
    if !text.contains('/') {
        return Ok(Cow::Borrowed(text));
    }
    let source = text.as_bytes();
    let mut result = source.to_vec();
    let mut at = 0;
    while at < source.len() {
        if matches!(source[at], b'\'' | b'"') {
            let quote = source[at];
            at += 1;
            while at < source.len() {
                if source[at] == b'\\' {
                    at = (at + 2).min(source.len());
                } else if source[at] == quote {
                    at += 1;
                    break;
                } else {
                    at += 1;
                }
            }
        } else if source[at..].starts_with(b"//") {
            result[at..].fill(b' ');
            break;
        } else if source[at..].starts_with(b"/*") {
            let end = memchr::memmem::find(&source[at + 2..], b"*/")
                .map(|n| at + n + 4)
                .ok_or_else(|| {
                    Error::input(
                        "source_syntax_error",
                        "unterminated conditional comment",
                        None,
                    )
                })?;
            result[at..end].fill(b' ');
            at = end;
        } else {
            at += 1;
        }
    }
    String::from_utf8(result)
        .map(Cow::Owned)
        .map_err(|e| Error::caused_by("invalid conditional expression", e))
}

fn unquoted(text: &str) -> impl Iterator<Item = (usize, u8)> + '_ {
    let mut quote = None;
    let mut escaped = false;
    text.bytes().enumerate().filter(move |(_, byte)| {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == delimiter {
                quote = None;
            }
            return false;
        }
        if matches!(*byte, b'\'' | b'"') {
            quote = Some(*byte);
            return false;
        }
        true
    })
}

fn identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || !byte.is_ascii()
}

fn canonical(text: &str) -> String {
    let mut result = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut pending = false;
    let mut previous_word = false;
    for character in text.chars() {
        if let Some(delimiter) = quote {
            result.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        if character.is_ascii_whitespace() {
            pending = true;
            continue;
        }
        let current_word = character.is_alphanumeric() || character == '_';
        if pending && previous_word && current_word {
            result.push(' ');
        }
        result.push(character);
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        }
        pending = false;
        previous_word = current_word;
    }
    result
}
