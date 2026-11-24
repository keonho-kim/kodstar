// Purpose: script·tag·brace 코드 영역 식별.
// Pattern: Simple Module.
// Usage: component adapter가 masking 범위를 요청.
// Related: crates/kodstar-core/src/infrastructure/formats/components.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::languages::Language;
use crate::domain::source::SourceSpan;

pub(super) fn find(source: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    source
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| start + offset)
}
pub(super) fn find_ascii_case(source: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    source
        .get(start..)?
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|i| start + i)
}
pub(super) fn find_line(source: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    let mut offset = start;
    for line in source[start..].split_inclusive(|&byte| byte == b'\n') {
        if line.trim_ascii() == needle {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}
pub(super) fn tag_end(source: &[u8], start: usize) -> Result<usize> {
    let mut quote = None;
    let mut braces = 0usize;
    for (index, &byte) in source.iter().enumerate().skip(start) {
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        } else if byte == b'{' {
            braces += 1;
        } else if byte == b'}' && braces > 0 {
            braces -= 1;
        } else if byte == b'>' && braces == 0 {
            return Ok(index);
        }
    }
    Err(Error::input(
        "source_syntax_error",
        "component opening tag is incomplete",
        None,
    ))
}
pub(in crate::infrastructure::formats) fn matching_brace(
    source: &[u8],
    start: usize,
) -> Result<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, &byte) in source.iter().enumerate().skip(start) {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth -= 1;
            if depth == 0 {
                return Ok(index);
            }
        }
    }
    Err(Error::input(
        "source_syntax_error",
        "Razor code body is incomplete",
        None,
    ))
}

pub(super) fn next_markup(
    source: &[u8],
    offset: &mut usize,
    format: Language,
    code: &[u8],
    cancel: &Cancellation,
) -> Result<Option<usize>> {
    while let Some(start) = source[*offset..]
        .iter()
        .position(|&b| {
            b == b'<'
                || b == b'@' && format == Language::Razor
                || b == b'{' && matches!(format, Language::Vue | Language::Svelte | Language::Astro)
        })
        .map(|i| *offset + i)
    {
        cancel.check()?;
        *offset = start + 1;
        if code.get(start) == Some(&1) || code.get(start) == source.get(start) {
            continue;
        }
        if source[start] == b'@' {
            if source[start..].starts_with(b"@*") {
                *offset = find(source, start + 2, b"*@")
                    .map(|i| i + 2)
                    .ok_or_else(|| {
                        Error::input("source_syntax_error", "Razor comment is incomplete", None)
                    })?;
            }
            continue;
        }
        if source[start] == b'{' {
            if format != Language::Vue || source.get(start + 1) == Some(&b'{') {
                *offset = matching_brace(source, start)? + 1;
            }
            continue;
        }
        if source[start..].starts_with(b"<!--") {
            *offset = find(source, start + 4, b"-->")
                .map(|i| i + 3)
                .ok_or_else(|| {
                    Error::input(
                        "source_syntax_error",
                        "component comment is incomplete",
                        None,
                    )
                })?;
            continue;
        }
        return Ok(Some(start));
    }
    Ok(None)
}

pub(super) struct Positions {
    offset: usize,
    line: usize,
    column: usize,
}
impl Positions {
    pub fn new() -> Self {
        Self {
            offset: 0,
            line: 1,
            column: 1,
        }
    }
    fn advance(&mut self, source: &[u8], end: usize) {
        for &byte in &source[self.offset..end] {
            if byte == b'\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
        self.offset = end;
    }
    pub fn span(&mut self, source: &[u8], range: std::ops::Range<usize>) -> SourceSpan {
        self.advance(source, range.start);
        let (line, column) = (self.line, self.column);
        self.advance(source, range.end);
        SourceSpan {
            start_byte: range.start,
            end_byte: range.end,
            line,
            column,
            end_line: self.line,
            end_column: self.column,
        }
    }
}

pub(super) fn blank(source: &[u8], mask: &mut [u8]) {
    for (slot, &byte) in mask.iter_mut().zip(source) {
        *slot = if matches!(byte, b'\r' | b'\n') {
            byte
        } else {
            b' '
        };
    }
}
