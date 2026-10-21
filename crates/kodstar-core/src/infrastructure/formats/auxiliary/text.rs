// Purpose: solution·form의 행 기반 관계 추출.
// Pattern: Format Adapter.
// Usage: auxiliary가 해당 보조 형식을 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs.

use std::path::Path;

use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Binding, Fact};
use crate::domain::relations::RelationKind;
use crate::domain::source::SourceSpan;

use super::usage;

pub(super) fn solution(source: &[u8], emit: &mut dyn FnMut(Fact) -> Result<()>) -> Result<()> {
    let content = std::str::from_utf8(source)
        .map_err(|error| Error::caused_by("non-UTF-8 solution file", error))?;
    for (line, location) in super::lines::lines(content) {
        if line.trim_start().starts_with("Project(")
            && let Some(path) = line.split('"').nth(5)
        {
            usage(
                format!("@file:{}", path.replace('\\', "/")),
                RelationKind::DependsOn,
                location,
                emit,
            )?;
        }
    }
    Ok(())
}

pub(super) fn form(
    path: &Path,
    source: &[u8],
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    let content = std::str::from_utf8(source)
        .map_err(|error| Error::caused_by("binary or unsupported form encoding", error))?;
    let unit = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let mut owner = String::new();
    for (line, location) in super::lines::lines(content) {
        let clean = line.trim();
        if owner.is_empty()
            && (clean.starts_with("object ") || clean.starts_with("inherited "))
            && let Some((_, ty)) = clean.split_once(':')
        {
            owner = ty.trim().into();
            emit(Fact::Binding(Binding {
                scope: 0,
                name: owner.clone(),
                target: None,
                module: Some(format!("@pascal-file:{unit}.pas")),
                imported_name: Some(owner.clone()),
                type_name: None,
                kind: "import",
                span: location,
            }))?;
        }
        if let Some((property, handler)) = clean.split_once('=')
            && property.trim().starts_with("On")
            && !handler.trim().is_empty()
            && handler
                .trim()
                .chars()
                .all(|character| character.is_alphanumeric() || character == '_')
        {
            usage(
                format!("{owner}::{}", handler.trim()),
                RelationKind::EventHandler,
                location,
                emit,
            )?;
        }
    }
    usage(
        format!("@file:{unit}.pas"),
        RelationKind::Includes,
        SourceSpan::at(source, 0, source.len()),
        emit,
    )
}
