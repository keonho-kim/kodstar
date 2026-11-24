// Purpose: EJS server와 개별 client script를 순차 파싱하고 전역/module scope 구성.
// Pattern: Format Adapter.
// Usage: component adapter가 EJS 파일의 실제 입력을 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/ejs/regions.rs, crates/kodstar-core/src/infrastructure/formats/components/script_facts.rs.

pub(super) mod regions;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Binding, Fact, Scope};
use crate::domain::languages::Language;
use crate::domain::source::SourceSpan;
use crate::infrastructure::formats::components::regions::{Positions, blank};
use crate::infrastructure::formats::components::script_facts;
use crate::infrastructure::formats::components::scripts::ScriptCursor;
use crate::infrastructure::parse::{ParseSession, ParseSyntax};
use std::path::Path;

pub(in crate::infrastructure::formats::components) fn extract(
    parser: &mut ParseSession,
    path: &Path,
    source: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
) -> Result<()> {
    let mut mask = vec![0; source.len()];
    blank(source, &mut mask);
    let mut maximum = 0;
    if regions::server(source, &mut mask, cancel)? {
        parser.extract_with_syntax(
            ParseSyntax {
                language: Language::JavaScript,
                jsx: false,
                ejs_server: true,
                original_source: None,
            },
            path,
            &mask,
            cancel,
            &mut |fact| {
                match &fact {
                    Fact::Scope(scope) => {
                        maximum = maximum.max(scope.local_id);
                        if scope.local_id == 0 {
                            return Ok(());
                        }
                    }
                    Fact::Definition(definition) => {
                        maximum = maximum.max(definition.local_id);
                        if definition.local_id == 0 {
                            return Ok(());
                        }
                    }
                    _ => {}
                }
                emit(Language::JavaScript, fact)
            },
        )?;
    }
    if memchr::memmem::find(source, b"hx-").is_some() {
        regions::exclude_tags(source, &mut mask, cancel)?;
        crate::infrastructure::formats::components::references::template_references(
            Language::Ejs,
            source,
            Language::JavaScript,
            &mask,
            cancel,
            emit,
        )?;
    }
    if crate::infrastructure::formats::components::regions::find_ascii_case(source, 0, b"<script")
        .is_none()
    {
        return Ok(());
    }
    blank(source, &mut mask);
    regions::exclude_tags(source, &mut mask, cancel)?;
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
    let mut scripts = ScriptCursor::new(Language::Ejs, Language::JavaScript);
    let mut positions = Positions::new();
    while let Some(script) = scripts.next(source, &mask, cancel)? {
        let original = &source[script.body.clone()];
        let masked = &mut mask[script.body.clone()];
        masked.copy_from_slice(original);
        let rendered = regions::neutralize_tags(original, masked, cancel)?;
        let origin = positions.span(source, script.body);
        let root = if script.module {
            maximum = script_facts::add(maximum, 1)?;
            emit(
                script.language,
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
        emit(
            script.language,
            Fact::Binding(Binding {
                scope: root,
                name: "@ejs-client".into(),
                target: None,
                module: None,
                imported_name: None,
                type_name: rendered.then(|| "rendered".into()),
                kind: "ejs_client",
                span: origin,
            }),
        )?;
        let offset = maximum;
        let earlier = parser.partial.take();
        let result = parser.extract_with_syntax(
            ParseSyntax {
                language: script.language,
                jsx: script.jsx,
                ejs_server: false,
                original_source: Some(original),
            },
            path,
            masked,
            cancel,
            &mut |mut fact| {
                if script_facts::client_fact(&mut fact, offset, root, origin, &mut maximum)? {
                    emit(script.language, fact)?;
                }
                Ok(())
            },
        ).map_err(|error| {
            let kind = error.kind;
            let mut input = error.input.clone();
            if let Some(issue) = &mut input { issue.line = issue.line.map(|line| origin.line + line.saturating_sub(1)); }
            let mut wrapped = Error::caused_by(format!(
                "client script starting at {}:{}:{} (parser positions relative to this body)",
                path.display(), origin.line, origin.column
            ), error);
            wrapped.kind = kind;
            wrapped.input = input;
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
