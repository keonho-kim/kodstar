// Purpose: Pascal uses 파일 경로를 위치 보존 masking하고 원래 경로를 경계에서 읽음.
// Pattern: Source Adapter.
// Usage: Pascal parser 입력과 unit import 추출기가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/infrastructure/parse/pascal/imports.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::borrow::Cow;

fn trivia(source: &[u8], mut at: usize) -> usize {
    loop {
        while source.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if source.get(at) == Some(&b'{') {
            at += 1;
            while source.get(at).is_some_and(|c| *c != b'}') {
                at += 1;
            }
            at = (at + 1).min(source.len());
        } else if source.get(at..at + 2) == Some(b"(*") {
            at += 2;
            while at < source.len() && source.get(at..at + 2) != Some(b"*)") {
                at += 1;
            }
            at = (at + 2).min(source.len());
        } else if source.get(at..at + 2) == Some(b"//") {
            while source.get(at).is_some_and(|c| *c != b'\n') {
                at += 1;
            }
        } else {
            return at;
        }
    }
}

fn string_end(source: &[u8], start: usize) -> Result<usize> {
    let mut at = start + 1;
    while let Some(&c) = source.get(at) {
        if c == b'\'' {
            if source.get(at + 1) == Some(&b'\'') {
                at += 2;
            } else {
                return Ok(at + 1);
            }
        } else {
            at += 1;
        }
    }
    Err(Error::operation("unterminated Pascal string"))
}

pub(in crate::infrastructure::parse) fn prepare<'a>(
    source: &'a [u8],
    cancel: &Cancellation,
) -> Result<Cow<'a, [u8]>> {
    let mut result = Cow::Borrowed(source);
    let mut at = 0;
    let mut uses = false;
    while at < source.len() {
        if at.is_multiple_of(4096) {
            cancel.check()?;
        }
        at = trivia(source, at);
        let Some(&c) = source.get(at) else {
            break;
        };
        if c == b'\'' {
            at = string_end(source, at)?;
            continue;
        }
        if c == b';' {
            uses = false;
            at += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = at;
            while source
                .get(at)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                at += 1;
            }
            let token = &source[start..at];
            if token.eq_ignore_ascii_case(b"uses") {
                uses = true;
            }
            if uses && token.eq_ignore_ascii_case(b"in") {
                let literal = trivia(source, at);
                if source.get(literal) == Some(&b'\'') {
                    let end = string_end(source, literal)?;
                    for byte in &mut result.to_mut()[start..end] {
                        if !matches!(*byte, b'\r' | b'\n') {
                            *byte = b' ';
                        }
                    }
                    at = end;
                }
            }
        } else {
            at += 1;
        }
    }
    cancel.check()?;
    Ok(result)
}

pub(in crate::infrastructure::parse) fn file_after(
    source: &[u8],
    start: usize,
    end: usize,
    limit: usize,
) -> Result<Option<String>> {
    let at = trivia(source, start);
    if source
        .get(at..at + 2)
        .is_none_or(|s| !s.eq_ignore_ascii_case(b"in"))
    {
        return Ok(None);
    }
    let literal = trivia(source, at + 2);
    if literal >= end || source.get(literal) != Some(&b'\'') {
        return Ok(None);
    }
    let finish = string_end(source, literal)?.min(end);
    if finish.saturating_sub(literal) > limit {
        return Err(Error::new(
            ErrorKind::Limit,
            "Pascal unit path exceeds fact_batch_bytes",
        ));
    }
    let raw = std::str::from_utf8(&source[literal + 1..finish - 1])
        .map_err(|e| Error::caused_by("invalid Pascal unit path encoding", e))?;
    Ok(Some(raw.replace("''", "'").replace('\\', "/")))
}
