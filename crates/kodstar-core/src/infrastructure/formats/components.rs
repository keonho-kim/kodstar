// Purpose: embedded 입력 추출과 원본 위치 환산.
// Pattern: Format Adapter.
// Usage: 분석 worker가 혼합 언어 입력을 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/regions.rs, crates/kodstar-core/src/infrastructure/formats/components/references.rs.

mod astro;
mod blade;
mod ejs;
mod events;
mod razor;
pub(in crate::infrastructure::formats) mod references;
pub(in crate::infrastructure::formats) mod regions;
mod script_facts;
mod scripts;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::Fact;
use crate::domain::languages::Language;
use crate::domain::source::SourceSpan;
use crate::infrastructure::parse::{ParseSession, ParseSyntax, file_root};
use references::template_references;
use scripts::copy_scripts;
use std::path::Path;

pub(crate) fn extract(
    parser: &mut ParseSession,
    format: Language,
    path: &Path,
    source: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
) -> Result<()> {
    std::str::from_utf8(source).map_err(|error| {
        Error::input(
            "source_encoding_error",
            format!("non-UTF-8 component input: {error}"),
            None,
        )
    })?;
    file_root(path, source, &mut |fact| emit(format, fact))?;
    if format == Language::Ejs {
        return crate::infrastructure::formats::components::ejs::extract(
            parser, path, source, cancel, emit,
        );
    }
    if format == Language::Astro {
        return astro::extract(parser, path, source, cancel, emit);
    }
    let prefix = if format == Language::Razor {
        "class __KodstarEmbedded {\n"
    } else if format == Language::Blade {
        "<?php\n"
    } else {
        ""
    };
    let suffix = if format == Language::Razor { "\n}" } else { "" };
    let mut syntactic = Vec::with_capacity(prefix.len() + source.len() + suffix.len());
    syntactic.extend_from_slice(prefix.as_bytes());
    syntactic.extend(source.iter().map(|&byte| {
        if matches!(byte, b'\n' | b'\r') {
            byte
        } else {
            b' '
        }
    }));
    syntactic.extend_from_slice(suffix.as_bytes());
    let masked = &mut syntactic[prefix.len()..prefix.len() + source.len()];
    let mut syntax = if format == Language::Razor {
        Language::CSharp
    } else if format == Language::Blade {
        Language::Php
    } else {
        Language::JavaScript
    };
    let mut parts = 0usize;
    let mut jsx = false;
    if format == Language::Blade {
        parts += crate::infrastructure::formats::components::blade::regions::prepare(
            source, masked, cancel,
        )?;
    } else if format == Language::Razor {
        parts += crate::infrastructure::formats::components::razor::prepare(
            path,
            source,
            masked,
            cancel,
            &mut |fact| emit(Language::CSharp, fact),
        )?;
    } else {
        let scripts = copy_scripts(format, source, masked, syntax, cancel)?;
        syntax = scripts.language;
        jsx = scripts.jsx;
        parts += scripts.parts;
    }
    template_references(format, source, syntax, masked, cancel, emit)?;
    let whole = SourceSpan::at(source, 0, source.len());
    let mut synthetic_owner = None;
    if parts > 0 {
        let earlier = parser.partial.take();
        let result = parser.extract_with_syntax(
            ParseSyntax {
                language: syntax,
                jsx,
                ejs_server: false,
                original_source: None,
            },
            path,
            &syntactic,
            cancel,
            &mut |mut fact| {
                match &mut fact {
                    Fact::Scope(scope) => {
                        scope.span = rebase(scope.span, prefix.len(), whole);
                    }
                    Fact::Definition(definition) => {
                        if definition.local_id == 0 {
                            return Ok(());
                        }
                        if definition.span.start_byte < prefix.len() {
                            synthetic_owner = Some(definition.local_id);
                            return Ok(());
                        }
                        definition.span = rebase(definition.span, prefix.len(), whole);
                        if definition.owner == synthetic_owner {
                            definition.owner = Some(0);
                        }
                        if let Some(name) = definition
                            .qualified_name
                            .strip_prefix("__KodstarEmbedded::")
                        {
                            definition.qualified_name = name.into();
                        }
                    }
                    Fact::Binding(binding) => {
                        if binding.target == synthetic_owner && synthetic_owner.is_some() {
                            return Ok(());
                        }
                        if format == Language::Razor && Some(binding.scope) == synthetic_owner {
                            binding.scope = 0;
                        }
                        binding.span = rebase(binding.span, prefix.len(), whole);
                    }
                    Fact::Use(usage) => {
                        if Some(usage.source) == synthetic_owner {
                            usage.source = 0;
                        }
                        usage.span = rebase(usage.span, prefix.len(), whole);
                    }
                    _ => {}
                }
                // Scope zero is already owned by the original file.
                if matches!(&fact,Fact::Scope(scope) if scope.local_id==0) {
                    return Ok(());
                }
                emit(syntax, fact)
            },
        );
        parser.remap_partial(earlier, |span| rebase(span, prefix.len(), whole));
        result?;
    }
    Ok(())
}

fn rebase(mut location: SourceSpan, prefix: usize, whole: SourceSpan) -> SourceSpan {
    if prefix == 0 {
        return location;
    }
    if location.start_byte < prefix {
        location.start_byte = 0;
        location.line = 1;
        location.column = 1;
    } else {
        location.start_byte -= prefix;
        location.line = location.line.saturating_sub(1).max(1);
    }
    if location.end_byte > prefix + whole.end_byte {
        location.end_byte = whole.end_byte;
        location.end_line = whole.end_line;
        location.end_column = whole.end_column;
    } else {
        location.end_byte = location.end_byte.saturating_sub(prefix);
        location.end_line = location.end_line.saturating_sub(1).max(1);
    }
    location
}
