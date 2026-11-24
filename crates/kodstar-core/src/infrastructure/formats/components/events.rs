// Purpose: component 속성 범위와 Vue·Svelte 정적 이벤트 handler 추출.
// Pattern: Streaming Scanner.
// Usage: script 영역 판별과 템플릿 참조 scanner가 시작 태그를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/references.rs, crates/kodstar-core/src/infrastructure/formats/components/regions.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use std::ops::Range;

use crate::infrastructure::formats::markup::{Attribute, attributes};

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
        let Attribute {
            name, value, range, ..
        } = attribute;
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
