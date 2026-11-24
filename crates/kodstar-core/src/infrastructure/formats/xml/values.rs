// Purpose: XML 문서 view에서 정확한 원문 이름·인용 값·요소 내용 범위 조회.
// Pattern: Borrowed Source View.
// Usage: XML·.NET 규칙이 decoder 정규화 전의 원문 계약을 사용.
// Related: crates/kodstar-core/src/infrastructure/formats/xml.rs, crates/kodstar-core/src/infrastructure/formats/dotnet.rs.

use crate::domain::diagnostics::{Error, Result};
use std::ops::Range;

pub(in crate::infrastructure::formats) fn text(source: &[u8], range: Range<usize>) -> Result<&str> {
    source
        .get(range)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .ok_or_else(|| Error::operation("invalid XML source range"))
}

pub(in crate::infrastructure::formats) fn attribute<'a>(
    attribute: roxmltree::Attribute<'_, '_>,
    source: &'a [u8],
) -> Result<(&'a str, Range<usize>)> {
    // Compute from the full attribute range: compact qname/equals lengths
    // cannot represent every long name or whitespace sequence.
    let range = attribute.range();
    let raw = text(source, range.clone())?;
    let name_end = raw
        .find(|c: char| c.is_ascii_whitespace() || c == '=')
        .ok_or_else(|| Error::operation("XML attribute has no value"))?;
    let value_start = raw[name_end..]
        .find(['\'', '"'])
        .map(|offset| name_end + offset)
        .ok_or_else(|| Error::operation("XML attribute has no quoted value"))?;
    Ok((&raw[..name_end], range.start + value_start..range.end))
}

pub(in crate::infrastructure::formats) fn content<'a>(
    node: roxmltree::Node<'_, '_>,
    source: &'a [u8],
) -> Option<&'a str> {
    let raw = text(source, node.range()).ok()?;
    let mut quote = None;
    let mut start = None;
    for (at, byte) in raw.bytes().enumerate() {
        match (quote, byte) {
            (Some(current), b) if current == b => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => {
                start = Some(at + 1);
                break;
            }
            _ => {}
        }
    }
    let end = raw.rfind("</")?;
    raw.get(start?..end).filter(|text| !text.is_empty())
}
