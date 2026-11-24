// Purpose: 구조 매크로 식별자·인자·뒤따르는 블록의 어휘 경계 확인.
// Pattern: Lexical Scanner.
// Usage: native 입력 adapter가 문자열과 주석을 건너뛰며 경계만 확인.
// Related: crates/kodstar-core/src/infrastructure/parse/input/native.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;

pub(super) fn macro_name(bytes: &[u8]) -> bool {
    bytes.len() > 1
        && bytes
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
        && (bytes[0].is_ascii_uppercase() || bytes[0] == b'_')
}

pub(super) fn control_keyword(name: &[u8]) -> bool {
    matches!(
        name,
        b"if"
            | b"else"
            | b"for"
            | b"while"
            | b"switch"
            | b"catch"
            | b"do"
            | b"return"
            | b"throw"
            | b"new"
            | b"delete"
            | b"sizeof"
            | b"alignof"
            | b"decltype"
            | b"static_assert"
    )
}

pub(super) fn previous_word(source: &[u8], at: usize) -> &[u8] {
    let prefix = source[..at].trim_ascii_end();
    let start = prefix
        .iter()
        .rposition(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        .map_or(0, |offset| offset + 1);
    &prefix[start..]
}

pub(super) fn statement_boundary(source: &[u8], at: usize) -> bool {
    let line = memchr::memrchr(b'\n', &source[..at]).map_or(0, |offset| offset + 1);
    source[line..at].iter().all(u8::is_ascii_whitespace)
        || matches!(
            source[..at].trim_ascii_end().last(),
            Some(b'{' | b'}' | b';')
        )
}

pub(super) fn standalone(source: &[u8], at: usize) -> bool {
    let line = memchr::memrchr(b'\n', &source[..at]).map_or(0, |n| n + 1);
    if !source[line..at].iter().all(u8::is_ascii_whitespace) {
        return false;
    }
    let mut end = at;
    while source
        .get(end)
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        end += 1;
    }
    while source
        .get(end)
        .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r'))
    {
        end += 1;
    }
    end == source.len() || source[end] == b'\n' || source[end..].starts_with(b"//")
}

pub(super) fn declaration_after(source: &[u8], end: usize) -> bool {
    let rest = source[end..].trim_ascii_start();
    for word in [
        b"template".as_slice(),
        b"class",
        b"struct",
        b"namespace",
        b"enum",
        b"using",
    ] {
        if let Some(tail) = rest.strip_prefix(word)
            && tail
                .first()
                .is_some_and(|b| b.is_ascii_whitespace() || *b == b'<')
        {
            return !matches!(word, b"class" | b"struct")
                || !tail.trim_ascii_start().starts_with(b"{");
        }
    }
    false
}

pub(super) fn directive_end(source: &[u8], mut at: usize) -> Option<usize> {
    if source[at] != b'#' {
        return None;
    }
    let start = memchr::memrchr(b'\n', &source[..at]).map_or(0, |n| n + 1);
    if !source[start..at].iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    loop {
        let end = memchr::memchr(b'\n', &source[at..]).map_or(source.len(), |n| at + n);
        let line = source[at..end].trim_ascii_end();
        if end == source.len() || !line.ends_with(b"\\") {
            return Some(end);
        }
        at = end + 1;
    }
}

pub(super) fn token_end(source: &[u8], at: usize) -> Option<usize> {
    if source[at..].starts_with(b"//") {
        return Some(memchr::memchr(b'\n', &source[at..]).map_or(source.len(), |n| at + n));
    }
    if source[at..].starts_with(b"/*") {
        return Some(
            memchr::memmem::find(&source[at + 2..], b"*/").map_or(source.len(), |n| at + n + 4),
        );
    }
    if source[at..].starts_with(b"R\"") {
        let start = at + 2;
        if let Some(length) = memchr::memchr(b'(', &source[start..]).filter(|n| *n <= 16) {
            let end = [
                b")".as_slice(),
                &source[start..start + length],
                b"\"".as_slice(),
            ]
            .concat();
            let body = start + length + 1;
            return Some(
                memchr::memmem::find(&source[body..], &end)
                    .map_or(source.len(), |n| body + n + end.len()),
            );
        }
    }
    let quote = *source.get(at)?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let mut end = at + 1;
    while end < source.len() {
        if source[end] == quote {
            return Some(end + 1);
        }
        end = (end + if source[end] == b'\\' { 2 } else { 1 }).min(source.len());
    }
    Some(source.len())
}

pub(super) fn call_end(
    source: &[u8],
    mut at: usize,
    cancel: &Cancellation,
) -> Result<Option<usize>> {
    while source
        .get(at)
        .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r'))
    {
        at += 1;
    }
    if source.get(at) == Some(&b'(') {
        let mut depth = 1usize;
        at += 1;
        while at < source.len() && depth != 0 {
            cancel.check()?;
            if let Some(end) = token_end(source, at) {
                at = end;
                continue;
            }
            match source[at] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            at += 1;
        }
        if depth != 0 {
            return Ok(None);
        }
        while source
            .get(at)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r'))
        {
            at += 1;
        }
    }
    Ok(Some(at))
}

pub(super) fn tail(
    source: &[u8],
    at: usize,
    semicolon: bool,
    cancel: &Cancellation,
) -> Result<bool> {
    let Some(at) = call_end(source, at, cancel)? else {
        return Ok(false);
    };
    if source.get(at) == Some(&b'\n') && continues_expression(source, at, cancel)? {
        return Ok(false);
    }
    Ok(at == source.len()
        || matches!(source[at], b'\n' | b'{')
        || semicolon && source[at] == b';'
        || source[at..].starts_with(b"//"))
}

pub(super) fn continues_expression(
    source: &[u8],
    end: usize,
    cancel: &Cancellation,
) -> Result<bool> {
    let Some(mut next) = call_end(source, end, cancel)? else {
        return Ok(false);
    };
    loop {
        cancel.check()?;
        next = source.len() - source[next..].trim_ascii_start().len();
        if source[next..].starts_with(b"//") || source[next..].starts_with(b"/*") {
            next = token_end(source, next).unwrap_or(source.len());
        } else {
            break;
        }
    }
    Ok(!source[next..].starts_with(b"[[")
        && source
            .get(next)
            .is_some_and(|b| b"+-*/%&|^?:=.,[)]<>".contains(b)))
}

pub(super) fn parameter_tail(source: &[u8], end: usize) -> bool {
    source[end..]
        .trim_ascii_start()
        .first()
        .is_some_and(|b| matches!(b, b',' | b')' | b'='))
}

pub(super) fn block_macro(
    source: &[u8],
    at: usize,
    end: usize,
    cancel: &Cancellation,
) -> Result<bool> {
    let previous = source[..at].trim_ascii_end().last().copied();
    if previous_word(source, at) == b"if" || previous == Some(b')') {
        return Ok(false);
    }
    let line = memchr::memrchr(b'\n', &source[..at]).map_or(0, |n| n + 1);
    if !source[line..at].iter().all(u8::is_ascii_whitespace)
        && !matches!(previous, Some(b'{' | b'}' | b';'))
    {
        return Ok(false);
    }
    let Some(after) = call_end(source, end, cancel)? else {
        return Ok(false);
    };
    Ok(source[after..].trim_ascii_start().starts_with(b"{"))
}
