// Purpose: Astro 서버 frontmatter·template와 브라우저 script를 분리해 순차 추출.
// Pattern: Format Adapter.
// Usage: component adapter가 Astro 입력을 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/scripts.rs, crates/kodstar-core/src/infrastructure/formats/components/script_facts.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Fact, Scope};
use crate::domain::languages::Language;
use crate::domain::source::SourceSpan;
use crate::infrastructure::formats::components::scripts::ScriptCursor;
use crate::infrastructure::formats::components::{regions, script_facts};
use crate::infrastructure::parse::{ParseSession, ParseSyntax};
use std::path::Path;

pub(super) fn extract(
    parser: &mut ParseSession,
    path: &Path,
    source: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
) -> Result<()> {
    let mut mask = vec![0; source.len()];
    regions::blank(source, &mut mask);
    let mut maximum = 0;
    let first = source
        .split_inclusive(|b| *b == b'\n')
        .next()
        .unwrap_or(&[]);
    if first.trim_ascii() == b"---" {
        let begin = first.len();
        let end = regions::find_line(source, begin, b"---")
            .ok_or_else(|| Error::operation("Astro frontmatter has no closing delimiter"))?;
        mask[begin..end].copy_from_slice(&source[begin..end]);
        parser.extract(Language::TypeScript, path, &mask, cancel, &mut |fact| {
            match &fact {
                Fact::Scope(value) => {
                    maximum = maximum.max(value.local_id);
                    if value.local_id == 0 {
                        return Ok(());
                    }
                }
                Fact::Definition(value) => {
                    maximum = maximum.max(value.local_id);
                    if value.local_id == 0 {
                        return Ok(());
                    }
                }
                _ => {}
            }
            emit(Language::TypeScript, fact)
        })?;
    }
    crate::infrastructure::formats::components::references::template_references(
        Language::Astro,
        source,
        Language::TypeScript,
        &mask,
        cancel,
        emit,
    )?;
    if regions::find_ascii_case(source, 0, b"<script").is_none() {
        return Ok(());
    }
    let classic = script_facts::add(maximum, 1)?;
    maximum = classic;
    emit(
        Language::JavaScript,
        Fact::Scope(Scope {
            local_id: classic,
            parent: None,
            span: SourceSpan::at(source, 0, source.len()),
        }),
    )?;
    let mut cursor = ScriptCursor::new(Language::Astro, Language::JavaScript);
    let mut positions = regions::Positions::new();
    while let Some(script) = cursor.next(source, &mask, cancel)? {
        let processed = !script.attributes;
        let language = if processed {
            Language::TypeScript
        } else {
            Language::JavaScript
        };
        let origin = positions.span(source, script.body.clone());
        let root = if processed || script.module {
            maximum = script_facts::add(maximum, 1)?;
            emit(
                language,
                Fact::Scope(Scope {
                    local_id: maximum,
                    parent: Some(classic),
                    span: origin,
                }),
            )?;
            maximum
        } else {
            classic
        };
        let original = &source[script.body.clone()];
        let masked = &mut mask[script.body];
        masked.copy_from_slice(original);
        let offset = maximum;
        let earlier = parser.partial.take();
        let result = parser.extract_with_syntax(
            ParseSyntax { language, jsx: false, ejs_server: false, original_source: None },
            path, masked, cancel, &mut |mut fact| {
                if script_facts::client_fact(&mut fact, offset, root, origin, &mut maximum)? {
                    emit(language, fact)?;
                }
                Ok(())
            },
        ).map_err(|error| {
            let kind = error.kind;
            let mut wrapped = Error::caused_by(format!(
                "client script starting at {}:{}:{} (parser positions relative to this body)",
                path.display(), origin.line, origin.column
            ), error);
            wrapped.kind = kind;
            wrapped
        });
        parser.remap_partial(earlier, |mut span| {
            script_facts::position(&mut span, origin);
            span
        });
        result?;
    }
    Ok(())
}
