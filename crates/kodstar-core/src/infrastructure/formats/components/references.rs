// Purpose: 코드·주석·style·속성 문자열을 제외한 template component·event 사용 추출.
// Pattern: Streaming Scanner.
// Usage: component adapter가 원본과 같은 길이의 코드 mask를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/events.rs, crates/kodstar-core/src/infrastructure/formats/components/blade/mod.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::facts::{Fact, SyntaxUse};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::formats::components::regions::tag_end;

pub(super) fn template_references(
    format: Language,
    source: &[u8],
    syntax: Language,
    code: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
) -> Result<()> {
    if format == Language::Blade {
        return crate::infrastructure::formats::components::blade::extract(
            source, code, cancel, emit,
        );
    }
    let mut positions = crate::infrastructure::formats::components::regions::Positions::new();
    let mut usage = |expression, relation, range: std::ops::Range<usize>| {
        emit(
            syntax,
            Fact::Use(SyntaxUse {
                source: 0,
                scope: 0,
                expression,
                relation,
                span: positions.span(source, range),
                condition: None,
                type_only: false,
            }),
        )
    };
    let mut offset = 0;
    while let Some(start) = crate::infrastructure::formats::components::regions::next_markup(
        source,
        &mut offset,
        format,
        code,
        cancel,
    )? {
        if !source.get(start + 1).is_some_and(u8::is_ascii_alphabetic) {
            continue;
        }
        let end = source[start + 1..]
            .iter()
            .position(|b| !b.is_ascii_alphanumeric() && !matches!(b, b'_' | b'.' | b':' | b'-'))
            .map_or(source.len(), |i| start + 1 + i);
        let name = std::str::from_utf8(&source[start + 1..end]).unwrap_or("");
        let closing = tag_end(source, end)?;
        offset = closing + 1;
        if name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style") {
            let marker = if name.eq_ignore_ascii_case("script") {
                b"</script".as_slice()
            } else {
                b"</style".as_slice()
            };
            offset = crate::infrastructure::formats::components::regions::find_ascii_case(
                source, offset, marker,
            )
            .map_or(source.len(), |i| i + marker.len());
            continue;
        }
        if name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            || format == Language::Vue && name.contains('-')
        {
            usage(
                format!("@component:{name}"),
                RelationKind::References,
                start + 1..end,
            )?;
        }
        crate::infrastructure::formats::components::events::extract(
            format,
            source,
            end..closing,
            &mut |name, range| usage(name.to_owned(), RelationKind::EventHandler, range),
        )?;
    }
    Ok(())
}
