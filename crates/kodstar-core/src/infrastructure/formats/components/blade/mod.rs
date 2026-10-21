// Purpose: Blade literal view 지시문과 PHP·주석 밖 markup 사용 사실 추출.
// Pattern: Format Adapter.
// Usage: template adapter가 원본과 PHP mask를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/blade/regions.rs, crates/kodstar-core/src/infrastructure/formats/components/blade/livewire.rs.

pub(super) mod livewire;
pub(super) mod regions;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::facts::{Fact, SyntaxUse};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::formats::components::regions::{Positions, find_ascii_case, tag_end};
use std::ops::Range;

pub(in crate::infrastructure::formats::components) fn extract(
    source: &[u8],
    code: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
) -> Result<()> {
    let mut positions = Positions::new();
    directives(source, code, cancel, &mut |expression, relation, range| {
        usage(source, &mut positions, emit, expression, relation, range)
    })?;
    positions = Positions::new();
    let mut at = 0usize;
    while at < source.len() {
        if at.is_multiple_of(4096) {
            cancel.check()?;
        }
        if code[at] == source[at] {
            at += 1;
            continue;
        }
        if regions::excluded(source, &mut at, cancel)? {
            continue;
        }
        if let Some(php) = regions::php(source, at, cancel)? {
            at = php.end;
            continue;
        }
        if source[at] == b'@' {
            at += 1;
            while source
                .get(at)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                at += 1;
            }
            while source.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            if source.get(at) == Some(&b'(') {
                at = regions::arguments_end(source, at, cancel)? + 1;
            }
            continue;
        }
        if source[at] != b'<' {
            at += 1;
            continue;
        }
        if source[at..].starts_with(b"<!--") {
            at = crate::infrastructure::formats::components::regions::find(source, at + 4, b"-->")
                .map_or(source.len(), |p| p + 3);
            continue;
        }
        if !source.get(at + 1).is_some_and(u8::is_ascii_alphabetic) {
            at += 1;
            continue;
        }
        let start = at + 1;
        let end = source[start..]
            .iter()
            .position(|b| !b.is_ascii_alphanumeric() && !matches!(b, b'_' | b'.' | b':' | b'-'))
            .map_or(source.len(), |i| start + i);
        let closing = tag_end(source, end)?;
        at = closing + 1;
        let name = &source[start..end];
        if name.eq_ignore_ascii_case(b"script") || name.eq_ignore_ascii_case(b"style") {
            let delimiter = if name.eq_ignore_ascii_case(b"script") {
                b"</script".as_slice()
            } else {
                b"</style".as_slice()
            };
            at = find_ascii_case(source, at, delimiter)
                .map_or(source.len(), |p| p + delimiter.len());
            continue;
        }
        crate::infrastructure::formats::components::blade::livewire::extract(
            source,
            start..end,
            end..closing,
            &mut |expression, relation, range| {
                usage(source, &mut positions, emit, expression, relation, range)
            },
        )?;
    }
    Ok(())
}
fn usage(
    source: &[u8],
    positions: &mut Positions,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
    expression: String,
    relation: RelationKind,
    range: Range<usize>,
) -> Result<()> {
    emit(
        Language::Php,
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
}
fn directives(
    source: &[u8],
    code: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(String, RelationKind, Range<usize>) -> Result<()>,
) -> Result<()> {
    let mut at = 0usize;
    while at < source.len() {
        if at.is_multiple_of(4096) {
            cancel.check()?;
        }
        if code[at] == source[at] {
            at += 1;
            continue;
        }
        if regions::excluded(source, &mut at, cancel)? {
            continue;
        }
        if let Some(php) = regions::php(source, at, cancel)? {
            at = php.end;
            continue;
        }
        if source[at] != b'@' {
            at += 1;
            continue;
        }
        let start = at;
        at += 1;
        while source
            .get(at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            at += 1;
        }
        let directive = &source[start + 1..at];
        while source.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if source.get(at) != Some(&b'(') {
            continue;
        }
        let finish = regions::arguments_end(source, at, cancel)?;
        let relation = match directive {
            b"extends" => RelationKind::TemplateExtends,
            b"include" | b"includeIf" | b"component" => RelationKind::TemplateInclude,
            _ => {
                at = finish + 1;
                continue;
            }
        };
        at += 1;
        while source.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let begin = at;
        let mut literal = None;
        if let Some(&quote @ (b'\'' | b'"')) = source.get(at) {
            at += 1;
            let text = at;
            while at < source.len() && source[at] != quote && source[at] != b'\\' {
                at += 1;
            }
            if source.get(at) == Some(&quote) {
                let end = at;
                at += 1;
                while source.get(at).is_some_and(u8::is_ascii_whitespace) {
                    at += 1;
                }
                let raw = std::str::from_utf8(&source[text..end]).unwrap_or("");
                if source.get(at).is_some_and(|b| matches!(b, b',' | b')'))
                    && !raw.is_empty()
                    && raw
                        .chars()
                        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
                {
                    literal = Some((raw, text..end));
                }
            }
        }
        if let Some((raw, range)) = literal {
            emit(
                format!(
                    "@rootfile:resources/views/{}.blade.php",
                    raw.replace('.', "/")
                ),
                relation,
                range,
            )?;
        } else {
            emit(
                "@blade-view:dynamic-or-unsupported".into(),
                relation,
                begin..finish,
            )?;
        }
        at = finish + 1;
    }
    Ok(())
}
