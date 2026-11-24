// Purpose: 문자열·주석과 구별한 C 계열 조건부 지시문 경계 읽기.
// Pattern: Lexical Scanner.
// Usage: conditional plan이 원본 바이트에 분기 구간을 생성.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/mod.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::ops::Range;

pub(super) enum Kind {
    If(String),
    ElseIf(String),
    Else,
    End,
}
pub(super) struct Directive {
    pub range: Range<usize>,
    pub kind: Kind,
}

pub(super) fn scan(source: &[u8], budget: usize, cancel: &Cancellation) -> Result<Vec<Directive>> {
    let mut output = Vec::new();
    let mut at = 0;
    let mut used = 0usize;
    while at < source.len() {
        cancel.check()?;
        let start = at;
        if source[at] == b'#' && line_prefix(source, at) {
            at += 1;
            let content = at;
            at = directive_end(source, at);
            let text = std::str::from_utf8(&source[content..at]).map_err(|_| {
                Error::input(
                    "source_encoding_error",
                    "invalid conditional directive encoding",
                    None,
                )
            })?;
            append(&mut output, start..at, text, &mut used, budget)?;
        } else if source[at..].starts_with(b"//") {
            at += memchr::memchr(b'\n', &source[at..]).unwrap_or(source.len() - at);
        } else if source[at..].starts_with(b"/*") {
            let close = b"*/";
            at += 2;
            at = memchr::memmem::find(&source[at..], close).map_or(source.len(), |n| at + n + 2);
        } else if matches!(source[at], b'\'' | b'"') {
            at = quoted(source, at);
        } else if source[at..].starts_with(b"R\"") {
            at += 2;
            let open = memchr::memchr(b'(', &source[at..]).filter(|n| *n <= 16);
            if let Some(length) = open {
                let closing =
                    [b")".as_slice(), &source[at..at + length], b"\"".as_slice()].concat();
                at += length + 1;
                at = memchr::memmem::find(&source[at..], &closing)
                    .map_or(source.len(), |n| at + n + closing.len());
            }
        } else {
            at += 1;
        }
    }
    Ok(output)
}

fn directive_end(source: &[u8], mut at: usize) -> usize {
    while at < source.len() {
        if source[at..].starts_with(b"/*") {
            at += 2;
            at = memchr::memmem::find(&source[at..], b"*/").map_or(source.len(), |n| at + n + 2);
        } else if source[at..].starts_with(b"//") {
            return at + memchr::memchr(b'\n', &source[at..]).unwrap_or(source.len() - at);
        } else if matches!(source[at], b'\'' | b'"') {
            at = quoted(source, at);
        } else if source[at] == b'\n' {
            let previous = at - usize::from(at > 0 && source[at - 1] == b'\r');
            if previous == 0 || source[previous - 1] != b'\\' {
                break;
            }
            at += 1;
        } else {
            at += 1;
        }
    }
    at
}

fn quoted(source: &[u8], mut at: usize) -> usize {
    let quote = source[at];
    let verbatim = at > 0 && source[at - 1] == b'@';
    let count = source[at..].iter().take_while(|b| **b == quote).count();
    if quote == b'"' && count >= 3 {
        at += count;
        return memchr::memmem::find(&source[at..], &vec![quote; count])
            .map_or(source.len(), |n| at + n + count);
    }
    at += 1;
    while at < source.len() {
        if source[at] == quote {
            at += 1;
            if verbatim && source.get(at) == Some(&quote) {
                at += 1;
            } else {
                break;
            }
        } else if !verbatim && source[at] == b'\\' {
            at = (at + 2).min(source.len());
        } else {
            at += 1;
        }
    }
    at
}

fn line_prefix(source: &[u8], at: usize) -> bool {
    let start = memchr::memrchr(b'\n', &source[..at]).map_or(0, |n| n + 1);
    source[start..at].iter().all(u8::is_ascii_whitespace)
}

fn append(
    output: &mut Vec<Directive>,
    range: Range<usize>,
    text: &str,
    used: &mut usize,
    budget: usize,
) -> Result<()> {
    let text = text.trim();
    let end = text
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    let word = text[..end].to_ascii_lowercase();
    let value = text[end..].trim();
    let value = value.replace("\\\r\n", " ").replace("\\\n", " ");
    let kind = match word.as_str() {
        "if" => Kind::If(value),
        "ifdef" => Kind::If(format!("defined({value})")),
        "ifndef" => Kind::If(format!("!defined({value})")),
        "elif" | "elseif" => Kind::ElseIf(value),
        "else" => Kind::Else,
        "endif" => Kind::End,
        _ => return Ok(()),
    };
    *used = used.saturating_add(text.len() + 256);
    if *used > budget {
        return Err(Error::new(
            ErrorKind::Limit,
            "conditional directive workspace exceeded",
        ));
    }
    output.push(Directive { range, kind });
    Ok(())
}
