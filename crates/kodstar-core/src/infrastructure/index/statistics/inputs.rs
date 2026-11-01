// Purpose: 정상 세대의 파일 상태·고유 객체 수를 물리 행 변화로 유지.
// Pattern: Transactional Delta Aggregate.
// Usage: 분석 transaction이 begin하고 통계 공개가 read·finish를 호출.
// Related: crates/kodstar-core/src/infrastructure/index/statistics/inputs.sql, crates/kodstar-core/src/infrastructure/index/statistics.rs.

use crate::domain::diagnostics::{Error, Result};
use rusqlite::Connection;

#[derive(Debug, PartialEq)]
pub(super) struct Counts {
    pub scanned: u64,
    pub analyzed: u64,
    pub skipped: u64,
    pub unsupported: u64,
    pub bytes: u64,
    pub symbols: u64,
}

pub(crate) fn begin(c: &Connection, eligible: bool) -> Result<()> {
    if !eligible {
        return Ok(());
    }
    c.execute_batch("CREATE TEMP TABLE statistics_inputs(scanned INTEGER,analyzed INTEGER,
        skipped INTEGER,unsupported INTEGER,bytes INTEGER,symbols INTEGER);
        INSERT INTO statistics_inputs SELECT c.scanned_files,c.analyzed_files,c.skipped_files,
            c.unsupported_files,c.source_bytes,c.symbols FROM coverage c JOIN metadata m ON m.generation=c.generation
            WHERE c.singleton=1 AND m.singleton=1;")?;
    c.execute_batch(include_str!("inputs.sql"))?;
    Ok(())
}

pub(super) fn read(c: &Connection) -> Result<Counts> {
    let tracked: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_schema WHERE name='statistics_inputs')",
        [],
        |r| r.get(0),
    )?;
    let sql = if tracked {
        "SELECT scanned,analyzed,skipped,unsupported,bytes,symbols FROM statistics_inputs"
    } else {
        "SELECT count(*),coalesce(sum(status='analyzed'),0),coalesce(sum(status='skipped'),0),
            coalesce(sum(status='unsupported'),0),coalesce(sum(CASE WHEN status='analyzed' THEN size ELSE 0 END),0),
            (SELECT count(DISTINCT key_blob) FROM symbols) FROM files"
    };
    let result = c.query_row(sql, [], |r| {
        Ok(Counts {
            scanned: r.get(0)?,
            analyzed: r.get(1)?,
            skipped: r.get(2)?,
            unsupported: r.get(3)?,
            bytes: r.get(4)?,
            symbols: r.get(5)?,
        })
    })?;
    if result
        .analyzed
        .saturating_add(result.skipped)
        .saturating_add(result.unsupported)
        > result.scanned
    {
        return Err(Error::operation(
            "incremental file coverage is inconsistent",
        ));
    }
    Ok(result)
}

pub(crate) fn finish(c: &Connection) -> Result<()> {
    c.execute_batch(
        "DROP TRIGGER IF EXISTS temp.statistics_file_insert;
        DROP TRIGGER IF EXISTS temp.statistics_file_delete;
        DROP TRIGGER IF EXISTS temp.statistics_file_update;
        DROP TRIGGER IF EXISTS temp.statistics_symbol_insert;
        DROP TRIGGER IF EXISTS temp.statistics_symbol_delete;
        DROP TRIGGER IF EXISTS temp.statistics_symbol_update;
        DROP TABLE IF EXISTS temp.statistics_inputs;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
