// Purpose: Livewire component 이름과 wire:click의 정적 action 이름 추출.
// Pattern: Simple Module.
// Usage: Blade markup scanner가 태그와 속성 범위를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/blade/mod.rs, crates/kodstar-core/src/infrastructure/formats/components/events.rs.

use crate::domain::diagnostics::Result;
use crate::domain::relations::RelationKind;
use std::ops::Range;

pub(in crate::infrastructure::formats::components) fn extract(
    source: &[u8],
    name: Range<usize>,
    attributes: Range<usize>,
    emit: &mut dyn FnMut(String, RelationKind, Range<usize>) -> Result<()>,
) -> Result<()> {
    let tag = std::str::from_utf8(&source[name.clone()]).unwrap_or("");
    if let Some(component) = tag.strip_prefix("livewire:")
        && !component.is_empty()
        && component
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        emit(
            format!("@livewire-component:{component}"),
            RelationKind::References,
            name.start + 9..name.end,
        )?;
    }
    crate::infrastructure::formats::components::events::attributes(
        source,
        attributes,
        &mut |attribute| {
            if attribute.name.split('.').next() != Some("wire:click") {
                return Ok(());
            }
            let length = attribute
                .value
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
            let method = &attribute.value[..length];
            let tail = attribute.value[length..].trim_start();
            if !method.is_empty()
                && !method.as_bytes()[0].is_ascii_digit()
                && (tail.is_empty() || tail.starts_with('('))
            {
                emit(
                    format!("@livewire-action:{method}"),
                    RelationKind::EventHandler,
                    attribute.range.start..attribute.range.start + length,
                )?;
            } else {
                emit(
                    "@livewire-action:<dynamic>".into(),
                    RelationKind::EventHandler,
                    attribute.range,
                )?;
            }
            Ok(())
        },
    )
}
