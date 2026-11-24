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

pub(in crate::infrastructure::formats) fn template_references(
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
    let htmx = memchr::memmem::find(source, b"hx-").is_some();
    let mut positions = crate::infrastructure::formats::components::regions::Positions::new();
    let mut scope = crate::infrastructure::formats::htmx::scope::Scope::default();
    let mut offset = 0;
    while let Some(start) = crate::infrastructure::formats::components::regions::next_markup(
        source,
        &mut offset,
        format,
        code,
        cancel,
    )? {
        let close = source.get(start + 1) == Some(&b'/');
        let begin = start + 1 + usize::from(close);
        if !source.get(begin).is_some_and(u8::is_ascii_alphabetic) {
            continue;
        }
        let end = source[begin..]
            .iter()
            .position(|b| !b.is_ascii_alphanumeric() && !matches!(b, b'_' | b'.' | b':' | b'-'))
            .map_or(source.len(), |i| begin + i);
        let name = std::str::from_utf8(&source[begin..end]).unwrap_or("");
        let closing = tag_end(source, end)?;
        offset = closing + 1;
        if close {
            scope.close(name);
            continue;
        }
        if [
            "script", "style", "textarea", "title", "xmp", "iframe", "noembed", "noframes",
        ]
        .iter()
        .any(|raw| name.eq_ignore_ascii_case(raw))
        {
            let marker = format!("</{name}");
            offset = crate::infrastructure::formats::components::regions::find_ascii_case(
                source,
                offset,
                marker.as_bytes(),
            )
            .map_or(source.len(), |i| i + marker.len());
            continue;
        }
        let disabled = htmx
            && scope.open(
                name,
                source,
                end..closing,
                source.get(closing.saturating_sub(1)) == Some(&b'/'),
            )?;
        let origin = positions.span(source, start + 1..start + 1);
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
        if !matches!(format, Language::Html | Language::Ejs)
            && (name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
                || format == Language::Vue && name.contains('-'))
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
        if htmx {
            crate::infrastructure::formats::htmx::attributes(
                source,
                end..closing,
                origin,
                crate::infrastructure::formats::htmx::Owner {
                    language: syntax,
                    source: 0,
                    scope: 0,
                    disabled,
                },
                Some(code),
                emit,
            )?;
        }
    }
    Ok(())
}
