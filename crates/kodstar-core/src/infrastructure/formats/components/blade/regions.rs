// Purpose: Blade 비코드 영역·escape와 PHP 본문을 원본 범위로 식별.
// Pattern: Streaming Scanner.
// Usage: Blade adapter가 PHP mask와 template 참조를 준비.
// Related: crates/kodstar-core/src/infrastructure/formats/components/blade/mod.rs, crates/kodstar-core/src/infrastructure/formats/components.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::infrastructure::formats::components::regions::find;

pub(in crate::infrastructure::formats::components) fn word(
    source: &[u8],
    at: usize,
    value: &[u8],
) -> bool {
    source[at..].starts_with(value)
        && !source
            .get(at + value.len())
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
}
pub(in crate::infrastructure::formats::components) fn excluded(
    source: &[u8],
    at: &mut usize,
    cancel: &Cancellation,
) -> Result<bool> {
    let (open, close) = if source[*at..].starts_with(b"{{--") {
        (b"{{--".as_slice(), b"--}}".as_slice())
    } else if word(source, *at, b"@verbatim") {
        (b"@verbatim".as_slice(), b"@endverbatim".as_slice())
    } else if source[*at..].starts_with(b"{{") {
        (b"{{".as_slice(), b"}}".as_slice())
    } else if source[*at..].starts_with(b"{!!") {
        (b"{!!".as_slice(), b"!!}".as_slice())
    } else if source[*at..].starts_with(b"@@") {
        *at += 2;
        return Ok(true);
    } else {
        return Ok(false);
    };
    *at = if matches!(open, b"{{" | b"{!!") {
        code_end(source, *at + open.len(), close, cancel)?.0 + close.len()
    } else {
        find(source, *at + open.len(), close)
            .map(|p| p + close.len())
            .ok_or_else(|| Error::operation("Blade comment/verbatim is incomplete"))?
    };
    Ok(true)
}
pub(in crate::infrastructure::formats::components) fn prepare(
    source: &[u8],
    mask: &mut [u8],
    cancel: &Cancellation,
) -> Result<usize> {
    let mut at = 0usize;
    let mut parts = 0;
    while at < source.len() {
        if at.is_multiple_of(4096) {
            cancel.check()?;
        }
        if excluded(source, &mut at, cancel)? {
            continue;
        }
        if let Some(region) = php(source, at, cancel)? {
            mask[region.body.clone()].copy_from_slice(&source[region.body.clone()]);
            if let Some(comment) = region.trailing_comment {
                crate::infrastructure::formats::components::regions::blank(
                    &source[comment..region.body.end],
                    &mut mask[comment..region.body.end],
                );
            }
            mask[region.body.end] = b';';
            parts += 1;
            at = region.end;
        } else {
            if source[at] == b'@' {
                at += 1;
                while source
                    .get(at)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                {
                    at += 1;
                }
                while source.get(at).is_some_and(u8::is_ascii_whitespace) {
                    at += 1;
                }
                if source.get(at) == Some(&b'(') {
                    at = arguments_end(source, at, cancel)? + 1;
                }
                continue;
            }
            at += 1;
            continue;
        }
    }
    Ok(parts)
}
pub(in crate::infrastructure::formats::components) fn arguments_end(
    source: &[u8],
    open: usize,
    cancel: &Cancellation,
) -> Result<usize> {
    code_end(source, open + 1, b")", cancel).map(|end| end.0)
}
pub(in crate::infrastructure::formats::components) struct Php {
    pub body: std::ops::Range<usize>,
    pub end: usize,
    trailing_comment: Option<usize>,
}
pub(in crate::infrastructure::formats::components) fn php(
    source: &[u8],
    at: usize,
    cancel: &Cancellation,
) -> Result<Option<Php>> {
    let (mut begin, mut close) = if word(source, at, b"@php") {
        (at + 4, b"@endphp".as_slice())
    } else if word(source, at, b"<?php") {
        (at + 5, b"?>".as_slice())
    } else {
        return Ok(None);
    };
    let mut next = begin;
    while source.get(next).is_some_and(u8::is_ascii_whitespace) {
        next += 1;
    }
    if close == b"@endphp" && source.get(next) == Some(&b'(') {
        begin = next + 1;
        close = b")";
    }
    let (end, trailing_comment) = code_end(source, begin, close, cancel)?;
    Ok(Some(Php {
        body: begin..end,
        end: end + close.len(),
        trailing_comment,
    }))
}
fn code_end(
    source: &[u8],
    mut at: usize,
    close: &[u8],
    cancel: &Cancellation,
) -> Result<(usize, Option<usize>)> {
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = None;
    let mut block_comment = false;
    let mut depth = usize::from(close == b")");
    while at < source.len() {
        if at.is_multiple_of(4096) {
            cancel.check()?;
        }
        let byte = source[at];
        let tail = &source[at..];
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active {
                quote = None;
            }
        } else if block_comment {
            if tail.starts_with(b"*/") {
                block_comment = false;
                at += 1;
            }
        } else if tail.starts_with(close) && (line_comment.is_none() || close == b"?>") {
            if close != b")" || depth == 1 {
                return Ok((at, line_comment));
            }
            depth -= 1;
        } else if line_comment.is_some() {
            if byte == b'\n' {
                line_comment = None;
            }
        } else if tail.starts_with(b"/*") {
            block_comment = true;
            at += 1;
        } else if tail.starts_with(b"//") || byte == b'#' && !tail.starts_with(b"#[") {
            line_comment = Some(at);
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        } else if close == b")" && byte == b'(' {
            depth += 1;
        }
        at += 1;
    }
    Err(Error::operation("Blade PHP body is incomplete"))
}
