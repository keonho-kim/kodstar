// Purpose: 공개 세대의 파일·관계 통계와 입력 진단 범주 조회.
// Pattern: Storage Adapter.
// Usage: analysis는 commit 전에 refresh하고 읽기 snapshot은 read를 호출.
// Related: crates/kodstar-core/src/infrastructure/index/schema.sql, crates/kodstar-core/src/application/analysis/mod.rs.

use crate::domain::diagnostics::{Coverage, Result};
use rusqlite::{Connection, OptionalExtension, params};
pub(crate) mod delta;
pub(crate) mod inputs;

#[derive(Clone, Copy)]
pub(crate) enum Update {
    Files,
    Selected,
    All,
}
#[cfg(test)]
mod tests;

pub(super) fn refresh(c: &Connection, generation: i64, update: Update) -> Result<()> {
    let input = inputs::read(c)?;
    let previous: Option<(i64, i64)> = if matches!(update, Update::All) {
        None
    } else {
        c.query_row("SELECT c.relationships,c.unresolved FROM coverage c JOIN metadata m ON m.generation=c.generation
            WHERE c.singleton=1 AND m.singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?))).optional()?
    };
    let retained = match (update, previous) {
        (Update::Files, prior) => prior,
        (Update::Selected, Some((relations, unresolved))) => delta::difference(c)?
            .map(|(a, b)| -> Result<_> {
                let relations = relations.checked_add(a).filter(|n| *n >= 0);
                let unresolved = unresolved.checked_add(b).filter(|n| *n >= 0);
                relations.zip(unresolved).ok_or_else(|| {
                    crate::domain::diagnostics::Error::operation(
                        "incremental relationship coverage is inconsistent",
                    )
                })
            })
            .transpose()?,
        _ => None,
    };
    let (relationships, unresolved) = match retained {
        Some(value) => value,
        None => c.query_row("SELECT (SELECT COUNT(*) FROM relationships WHERE resolution='resolved'),
            (SELECT COUNT(DISTINCT fact_id) FROM relationships WHERE resolution IN ('unresolved','ambiguous'))",
            [], |r| Ok((r.get(0)?, r.get(1)?)))?,
    };
    c.execute(
        "INSERT OR REPLACE INTO coverage VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            generation,
            input.scanned,
            input.analyzed,
            input.skipped,
            input.unsupported,
            input.bytes,
            input.symbols,
            relationships,
            unresolved
        ],
    )?;
    inputs::finish(c)
}

pub(super) fn read(connection: &Connection) -> Result<Coverage> {
    connection
        .query_row(
            "SELECT c.scanned_files,c.analyzed_files,c.skipped_files,c.unsupported_files,
                c.source_bytes,c.symbols,c.relationships,c.unresolved
         FROM coverage c JOIN metadata m ON m.generation=c.generation
         WHERE c.singleton=1 AND m.singleton=1",
            [],
            |row| {
                Ok(Coverage {
                    scanned_files: row.get(0)?,
                    analyzed_files: row.get(1)?,
                    skipped_files: row.get(2)?,
                    unsupported_files: row.get(3)?,
                    source_bytes: row.get(4)?,
                    symbols: row.get(5)?,
                    relationships: row.get(6)?,
                    unresolved: row.get(7)?,
                    reused_files: 0,
                    failed_files: 0,
                    partial_files: 0,
                    fragment_files: 0,
                    configuration_incomplete_files: 0,
                    native_incomplete_files: 0,
                    native_limited_files: 0,
                })
            },
        )
        .map(|mut coverage| {
            coverage.failed_files = super::failures::count(connection)?;
            coverage.partial_files = connection.query_row("SELECT COUNT(DISTINCT file_id) FROM diagnostics WHERE code IN ('source_syntax_partial','source_macro_incomplete')", [], |r| r.get(0))?;
            coverage.fragment_files = connection.query_row("SELECT COUNT(DISTINCT file_id) FROM diagnostics WHERE code IN ('source_fragment_context','unexpanded_native_macros')", [], |r| r.get(0))?;
            coverage.configuration_incomplete_files = connection.query_row("SELECT COUNT(DISTINCT file_id) FROM diagnostics WHERE code IN ('configuration_incomplete','external_configuration')", [], |r| r.get(0))?;
            (coverage.native_incomplete_files, coverage.native_limited_files) = connection.query_row(
                "SELECT COALESCE(SUM(status='incomplete'),0),COALESCE(SUM(status='limited'),0) FROM native_analysis", [], |r| Ok((r.get(0)?,r.get(1)?)))?;
            Ok::<_, crate::domain::diagnostics::Error>(coverage)
        })?
}
