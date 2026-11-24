// Purpose: 제한된 Markdown 본문에서 첫 제목 또는 유일한 제목 anchor의 원문 위치 선택.
// Pattern: Simple Module.
// Usage: documentation query가 보존한 UTF-8 본문과 명시 anchor를 전달.
// Related: crates/kodstar-core/src/application/query/documentation.rs.

use crate::domain::source::SourceSpan;

pub(super) fn heading(source: &str, anchor: Option<&str>) -> Result<SourceSpan, &'static str> {
    let mut offset = 0;
    let mut fence = None;
    let mut matched = None;
    for raw in source.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim_start_matches(['\u{feff}', ' ']);
        let start = offset + line.len() - trimmed.len();
        offset += raw.len();
        let marker = trimmed.as_bytes().first().copied();
        if let Some(marker) = marker.filter(|m| matches!(*m, b'`' | b'~')) {
            let width = trimmed.bytes().take_while(|c| *c == marker).count();
            if width >= 3 {
                if let Some((open, size)) = fence {
                    if open == marker && width >= size && trimmed[width..].trim().is_empty() {
                        fence = None;
                    }
                } else {
                    fence = Some((marker, width));
                }
                continue;
            }
        }
        if fence.is_some() {
            continue;
        }
        let count = trimmed.bytes().take_while(|c| *c == b'#').count();
        if !(1..=6).contains(&count) || !trimmed[count..].starts_with([' ', '\t']) {
            continue;
        }
        if anchor.is_some_and(|anchor| {
            anchor != slug(trimmed[count..].trim().trim_end_matches('#').trim_end())
        }) {
            continue;
        }
        let span = SourceSpan::at(source.as_bytes(), start, start + trimmed.len());
        if anchor.is_none() {
            return Ok(span);
        }
        if matched.replace(span).is_some() {
            return Err("ambiguous_document_anchor");
        }
    }
    if anchor.is_none() {
        return Ok(SourceSpan::at(source.as_bytes(), 0, 0));
    }
    matched.ok_or("document_anchor_not_found")
}

fn slug(title: &str) -> String {
    title
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(|c| {
            if c.is_whitespace() {
                Some('-')
            } else {
                (c.is_alphanumeric() || matches!(c, '-' | '_')).then_some(c)
            }
        })
        .collect()
}
