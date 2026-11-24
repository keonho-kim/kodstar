// Purpose: solution의 행 기반 관계 추출.
// Pattern: Format Adapter.
// Usage: auxiliary가 해당 보조 형식을 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs.

use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::Fact;
use crate::domain::relations::RelationKind;

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
