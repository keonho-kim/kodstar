// Purpose: Go module·workspace 지시문을 정적 의존·경로·버전 사실로 추출.
// Pattern: Format Adapter.
// Usage: auxiliary dispatcher가 go.mod·go.work를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/go_tokens.rs, crates/kodstar-core/src/application/analysis/resolve/go/mod.rs.

use super::{go_tokens::tokens, usage};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Binding, Fact};
use crate::domain::relations::RelationKind;
use crate::domain::source::SourceSpan;
use std::path::Path;

pub(super) fn extract(
    path: &Path,
    source: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    let content =
        std::str::from_utf8(source).map_err(|e| Error::caused_by("non-UTF-8 Go manifest", e))?;
    let mut block = String::new();
    for (line, at) in super::lines::lines(content) {
        cancel.check()?;
        let words = tokens(line)?;
        let Some(first) = words.first() else {
            continue;
        };
        if first == ")" {
            if block.is_empty() || words.len() != 1 {
                return Err(Error::operation("unexpected Go manifest block end"));
            }
            block.clear();
            continue;
        }
        let (directive, values) = if block.is_empty() {
            (first.as_str(), &words[1..])
        } else {
            (block.as_str(), words.as_slice())
        };
        if values == ["("] {
            if !block.is_empty() {
                return Err(Error::operation("nested Go manifest block"));
            }
            block = directive.into();
            continue;
        }
        match directive {
            "module" => {
                let [name] = values else {
                    return Err(Error::operation("invalid Go module directive"));
                };
                emit(Fact::Module {
                    name: name.clone(),
                    declaration_only: true,
                })?;
            }
            "use" => {
                let [directory] = values else {
                    return Err(Error::operation("invalid Go workspace use"));
                };
                metadata(
                    "go_workspace_use",
                    directory,
                    Some(directory),
                    None,
                    None,
                    at,
                    emit,
                )?;
                usage(
                    format!("@file:{directory}/go.mod"),
                    RelationKind::Includes,
                    at,
                    emit,
                )?;
            }
            "require" => {
                let [module, version] = values else {
                    return Err(Error::operation("invalid Go require directive"));
                };
                metadata("go_require", module, Some(version), None, None, at, emit)?;
                usage(
                    format!("@package:go:{module}"),
                    RelationKind::DependsOn,
                    at,
                    emit,
                )?;
            }
            "replace" => {
                let Some(arrow) = values.iter().position(|s| s == "=>") else {
                    return Err(Error::operation("missing Go replace arrow"));
                };
                let (from, to) = (&values[..arrow], &values[arrow + 1..]);
                if !(1..=2).contains(&from.len()) || !(1..=2).contains(&to.len()) {
                    return Err(Error::operation("invalid Go replacement"));
                }
                metadata(
                    "go_replace",
                    &from[0],
                    Some(&to[0]),
                    from.get(1).map(String::as_str),
                    to.get(1).map(String::as_str),
                    at,
                    emit,
                )?;
                if to.len() == 1 {
                    usage(
                        format!("@file:{}/go.mod", to[0]),
                        RelationKind::Includes,
                        at,
                        emit,
                    )?;
                } else {
                    usage(
                        format!("@package:go:{}", to[0]),
                        RelationKind::DependsOn,
                        at,
                        emit,
                    )?;
                }
            }
            _ => {}
        }
    }
    if !block.is_empty() {
        return Err(Error::operation(format!(
            "unterminated Go block in {}",
            path.display()
        )));
    }
    Ok(())
}

fn metadata(
    kind: &'static str,
    name: &str,
    module: Option<&str>,
    imported: Option<&str>,
    ty: Option<&str>,
    at: SourceSpan,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    emit(Fact::Binding(Binding {
        scope: 0,
        name: name.into(),
        target: None,
        module: module.map(str::to_owned),
        imported_name: imported.map(str::to_owned),
        type_name: ty.map(str::to_owned),
        kind,
        span: at,
    }))
}
