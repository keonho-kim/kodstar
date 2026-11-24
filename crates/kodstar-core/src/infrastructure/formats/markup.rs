// Purpose: 웹 입력 속성의 값과 원본 바이트 범위를 스트리밍으로 읽음.
// Pattern: Lexical Scanner.
// Usage: component script·event와 htmx adapter가 호출.
// Related: crates/kodstar-core/src/infrastructure/formats/components/events.rs, crates/kodstar-core/src/infrastructure/formats/htmx/mod.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::formats::components::regions::matching_brace;
use std::ops::Range;

pub(in crate::infrastructure) struct Attribute<'a> {
    pub name: &'a str,
    pub value: &'a str,
    pub raw: &'a str,
    pub expression: bool,
    pub range: Range<usize>,
}

pub(in crate::infrastructure) fn attributes(
    source: &[u8],
    range: Range<usize>,
    emit: &mut dyn FnMut(Attribute<'_>) -> Result<()>,
) -> Result<()> {
    let mut at = range.start;
    while at < range.end {
        while at < range.end && source[at].is_ascii_whitespace() {
            at += 1;
        }
        let start = at;
        while at < range.end
            && !source[at].is_ascii_whitespace()
            && !matches!(source[at], b'=' | b'/' | b'>')
        {
            at += 1;
        }
        if start == at {
            at += 1;
            continue;
        }
        let name = std::str::from_utf8(&source[start..at]).unwrap_or("");
        let name_end = at;
        while at < range.end && source[at].is_ascii_whitespace() {
            at += 1;
        }
        if source.get(at) != Some(&b'=') {
            emit(Attribute {
                name,
                value: "",
                raw: "",
                expression: false,
                range: name_end..name_end,
            })?;
            continue;
        }
        at += 1;
        while at < range.end && source[at].is_ascii_whitespace() {
            at += 1;
        }
        let begin;
        let end;
        let expression = source.get(at) == Some(&b'{');
        match source.get(at) {
            Some(b'\'' | b'"') => {
                let quote = source[at];
                begin = at + 1;
                end = source[begin..range.end]
                    .iter()
                    .position(|&b| b == quote)
                    .map_or(range.end, |i| begin + i);
                at = end + 1;
            }
            Some(b'{') => {
                begin = at + 1;
                end = matching_brace(source, at)?;
                at = end + 1;
            }
            _ => {
                begin = at;
                while at < range.end && !source[at].is_ascii_whitespace() && source[at] != b'>' {
                    at += 1;
                }
                end = at;
            }
        }
        if end <= range.end {
            let raw = std::str::from_utf8(&source[begin..end]).unwrap_or("");
            let value = raw.trim_ascii();
            let leading = raw.len() - raw.trim_ascii_start().len();
            emit(Attribute {
                name,
                value,
                raw,
                expression,
                range: begin + leading..begin + leading + value.len(),
            })?;
        }
    }
    Ok(())
}
