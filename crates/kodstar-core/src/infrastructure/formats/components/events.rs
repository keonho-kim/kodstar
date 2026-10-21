// Purpose: component 속성 범위와 Vue·Svelte 정적 이벤트 handler 추출.
// Pattern: Streaming Scanner.
// Usage: script 영역 판별과 템플릿 참조 scanner가 시작 태그를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/references.rs, crates/kodstar-core/src/infrastructure/formats/components/regions.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::formats::components::regions::matching_brace;
use std::ops::Range;

pub(super) struct Attribute<'a> {
    pub name: &'a str,
    pub value: &'a str,
    pub range: Range<usize>,
}

pub(super) fn extract(
    format: Language,
    source: &[u8],
    range: Range<usize>,
    emit: &mut dyn FnMut(&str, Range<usize>) -> Result<()>,
) -> Result<()> {
    if !matches!(format, Language::Vue | Language::Svelte | Language::Razor) {
        return Ok(());
    }
    attributes(source, range, &mut |attribute| {
        let Attribute { name, value, range } = attribute;
        let event = if format == Language::Vue {
            name.starts_with('@') || name.starts_with("v-on:")
        } else if format == Language::Razor {
            name.starts_with("@on")
        } else {
            name.starts_with("on")
        };
        let (value, range) = if format == Language::Razor && value.starts_with('@') {
            (&value[1..], range.start + 1..range.end)
        } else {
            (value, range)
        };
        if event
            && !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '.'))
        {
            emit(value, range)?;
        }
        Ok(())
    })
}

pub(super) fn attributes(
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
            let value = raw.trim();
            let leading = raw.len() - raw.trim_start().len();
            emit(Attribute {
                name,
                value,
                range: begin + leading..begin + leading + value.len(),
            })?;
        }
    }
    Ok(())
}
