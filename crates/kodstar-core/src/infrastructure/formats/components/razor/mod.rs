// Purpose: Razor 코드·주석·태그 영역과 정적 행 지시문을 한 번에 판별.
// Pattern: Streaming Scanner.
// Usage: component adapter가 C# parser 입력 mask를 만들 때 호출.
// Related: crates/kodstar-core/src/infrastructure/formats/components/razor/directives.rs, crates/kodstar-core/src/infrastructure/formats/components/regions.rs.

pub(super) mod directives;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Binding, Fact};
use crate::domain::source::SourceSpan;
use crate::infrastructure::formats::components::regions::{Positions, find, tag_end};
use std::path::Path;

pub(in crate::infrastructure::formats::components) fn prepare(
    path: &Path,
    source: &[u8],
    mask: &mut [u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<usize> {
    if path.extension().is_some_and(|s| s == "razor")
        && let Some(name) = path.file_stem().and_then(|s| s.to_str())
    {
        emit(Fact::Binding(Binding {
            scope: 0,
            name: "@razor-component".into(),
            target: Some(0),
            module: None,
            imported_name: Some(name.into()),
            type_name: None,
            kind: "razor_component",
            span: SourceSpan::at(source, 0, 0),
        }))?;
    }
    let mut offset = 0;
    let mut parts = 0;
    let mut positions = Positions::new();
    let mut checked_line = 0;
    while let Some(at) = source[offset..]
        .iter()
        .position(|b| matches!(b, b'@' | b'<'))
        .map(|i| offset + i)
    {
        cancel.check()?;
        offset = at + 1;
        let tail = &source[at..];
        let comment = if tail.starts_with(b"@*") {
            Some((2, b"*@".as_slice()))
        } else if tail.starts_with(b"<!--") {
            Some((4, b"-->".as_slice()))
        } else {
            None
        };
        if let Some((length, close)) = comment {
            offset = find(source, at + length, close)
                .map(|i| i + close.len())
                .ok_or_else(|| Error::operation("Razor comment is incomplete"))?;
            continue;
        }
        if tail.starts_with(b"@@") {
            offset = at + 2;
            continue;
        }
        if source[at] == b'<' {
            let end = tag_end(source, at + 1)?;
            let name_end = source[at + 1..end]
                .iter()
                .position(u8::is_ascii_whitespace)
                .map_or(end, |i| at + 1 + i);
            let name = &source[at + 1..name_end];
            offset = end + 1;
            if matches!(name, b"script" | b"style") {
                let close = if name == b"script" {
                    b"</script".as_slice()
                } else {
                    b"</style".as_slice()
                };
                let closing = find(source, offset, close)
                    .ok_or_else(|| Error::operation("Razor script/style is incomplete"))?;
                offset = tag_end(source, closing + close.len())? + 1;
            }
            continue;
        }
        let mut end = at + 1;
        while source.get(end).is_some_and(u8::is_ascii_alphabetic) {
            end += 1;
        }
        let name = std::str::from_utf8(&source[at + 1..end]).unwrap_or("");
        let mut begin = end;
        while source.get(begin).is_some_and(u8::is_ascii_whitespace) {
            begin += 1;
        }
        if matches!(name, "code" | "functions") && source.get(begin) == Some(&b'{')
            || tail.starts_with(b"@{")
        {
            let brace = if tail.starts_with(b"@{") {
                at + 1
            } else {
                begin
            };
            let closing = code_end(source, brace, cancel)?;
            if name == "code" || name == "functions" {
                mask[brace + 1..closing].copy_from_slice(&source[brace + 1..closing]);
                parts += 1;
            }
            offset = closing + 1;
            continue;
        }
        let point = positions.span(source, at..at);
        if point.line == checked_line {
            continue;
        }
        checked_line = point.line;
        let line_start = at - (point.column - 1);
        if !source[line_start..at].iter().all(u8::is_ascii_whitespace)
            || !matches!(
                name,
                "using" | "inject" | "model" | "inherits" | "page" | "namespace"
            )
        {
            continue;
        }
        let line_end = find(source, end, b"\n").unwrap_or(source.len());
        begin = end;
        while begin < line_end && source[begin].is_ascii_whitespace() {
            begin += 1;
        }
        let raw = std::str::from_utf8(&source[begin..line_end])
            .unwrap_or("")
            .trim_end();
        let span = positions.span(source, begin..begin + raw.len());
        crate::infrastructure::formats::components::razor::directives::extract(
            name, raw, span, emit,
        )?;
        offset = line_end;
    }
    Ok(parts)
}

fn code_end(source: &[u8], start: usize, cancel: &Cancellation) -> Result<usize> {
    let mut depth = 1usize;
    let mut at = start + 1;
    while at < source.len() {
        if at.is_multiple_of(4096) {
            cancel.check()?;
        }
        if source[at..].starts_with(b"//") {
            at = find(source, at + 2, b"\n").unwrap_or(source.len());
            continue;
        }
        if source[at..].starts_with(b"/*") {
            at = find(source, at + 2, b"*/")
                .map(|i| i + 2)
                .ok_or_else(|| Error::operation("Razor C# comment is incomplete"))?;
            continue;
        }
        if matches!(source[at], b'\'' | b'"') {
            let quote = source[at];
            let count = source[at..].iter().take_while(|&&b| b == quote).count();
            if quote == b'"' && count >= 3 {
                at = find(source, at + count, &source[at..at + count])
                    .map(|i| i + count)
                    .ok_or_else(|| Error::operation("Razor raw string is incomplete"))?;
                continue;
            }
            let verbatim = quote == b'"' && at > 0 && source[at - 1] == b'@';
            at += 1;
            while at < source.len() {
                if source[at] == quote {
                    at += 1;
                    if verbatim && source.get(at) == Some(&quote) {
                        at += 1;
                        continue;
                    }
                    break;
                }
                if !verbatim && source[at] == b'\\' {
                    at += 1;
                }
                at += 1;
            }
            continue;
        }
        if source[at] == b'{' {
            depth += 1;
        } else if source[at] == b'}' {
            depth -= 1;
            if depth == 0 {
                return Ok(at);
            }
        }
        at += 1;
    }
    Err(Error::operation("Razor code body is incomplete"))
}
