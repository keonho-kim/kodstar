// Purpose: pnpm workspace의 선정된 block packages 목록 추출.
// Pattern: Format Scanner.
// Usage: auxiliary adapter가 pnpm-workspace.yaml/yml 원본을 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs, crates/kodstar-core/src/application/analysis/resolve/javascript/workspaces.rs.

use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Binding, Fact};
use crate::domain::source::SourceSpan;

pub(super) fn extract(source: &[u8], emit: &mut dyn FnMut(Fact) -> Result<()>) -> Result<()> {
    let content = std::str::from_utf8(source)
        .map_err(|e| Error::caused_by("non-UTF-8 workspace manifest", e))?;
    let mut active = false;
    emit(Fact::Binding(Binding {
        scope: 0,
        name: "pnpm".into(),
        target: None,
        module: None,
        imported_name: None,
        type_name: None,
        kind: "workspace_root",
        span: SourceSpan::at(source, 0, 0),
    }))?;
    for (line, location) in super::lines::lines(content) {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("packages:") {
            if !value.trim().is_empty() && !value.trim().starts_with('#') && value.trim() != "[]" {
                return Err(Error::operation(
                    "workspace packages requires the supported block list form",
                ));
            }
            active = true;
        } else if active && let Some(value) = trimmed.strip_prefix('-') {
            let value = value.trim();
            let pattern = if value.starts_with('"') {
                let mut end = None;
                let mut escaped = false;
                for (index, byte) in value.bytes().enumerate().skip(1) {
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        end = Some(index + 1);
                        break;
                    }
                }
                let end = end.ok_or_else(|| Error::operation("unterminated workspace pattern"))?;
                serde_json::from_str::<String>(&value[..end])
                    .map_err(|e| Error::caused_by("decode workspace pattern", e))?
            } else if let Some(raw) = value.strip_prefix('\'') {
                let end = raw
                    .rfind('\'')
                    .ok_or_else(|| Error::operation("unterminated workspace pattern"))?;
                raw[..end].replace("''", "'")
            } else {
                value.split(" #").next().unwrap_or("").trim().to_string()
            };
            if !pattern.is_empty() {
                emit(Fact::Binding(Binding {
                    scope: 0,
                    name: pattern,
                    target: None,
                    module: None,
                    imported_name: None,
                    type_name: None,
                    kind: "workspace_glob",
                    span: location,
                }))?;
            }
        } else if active
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && !line.starts_with([' ', '\t'])
        {
            active = false;
        }
    }
    Ok(())
}
