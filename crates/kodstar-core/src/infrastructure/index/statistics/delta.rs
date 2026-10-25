// Purpose: 작은 증분 변경의 이전·이후 fact 통계 차이를 같은 transaction에서 계산.
// Pattern: Bounded Journal.
// Usage: 분석 writer가 삭제 전에 기록하고 통계 공개가 새 파생 fact까지 대조.
// Related: crates/kodstar-core/src/infrastructure/index/statistics.rs, crates/kodstar-core/src/application/analysis/pipeline.rs.

use crate::domain::diagnostics::Result;
use rusqlite::{Connection, params};

const LIMIT: i64 = 128;
#[cfg(test)]
mod tests;

pub(crate) fn reset(c: &Connection, eligible: bool) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS statistics_delta(enabled INTEGER NOT NULL);
        CREATE TEMP TABLE IF NOT EXISTS statistics_previous(id INTEGER PRIMARY KEY,resolved INTEGER,bad INTEGER);
        CREATE TEMP TABLE IF NOT EXISTS statistics_pending(id INTEGER PRIMARY KEY);
        DELETE FROM statistics_delta; DELETE FROM statistics_previous; DELETE FROM statistics_pending;")?;
    let large = if eligible {
        let prior = super::read(c)?;
        prior.relationships.saturating_add(prior.unresolved) >= (LIMIT * 4) as u64
    } else {
        false
    };
    c.execute("INSERT INTO statistics_delta VALUES(?1)", [large])?;
    Ok(())
}

fn enabled(c: &Connection) -> Result<bool> {
    Ok(c.query_row("SELECT enabled FROM statistics_delta", [], |r| r.get(0))?)
}

fn disable(c: &Connection) -> Result<()> {
    c.execute_batch("UPDATE statistics_delta SET enabled=0; DELETE FROM statistics_previous; DELETE FROM statistics_pending;")?;
    Ok(())
}

pub(crate) fn remember_file(c: &Connection, file: i64) -> Result<()> {
    if !enabled(c)? {
        return Ok(());
    }
    c.execute(
        "INSERT OR IGNORE INTO statistics_pending SELECT id FROM facts WHERE file_id=?1 LIMIT ?2",
        params![file, LIMIT + 1],
    )?;
    remember(c)
}

pub(crate) fn remember_deleted(c: &Connection) -> Result<()> {
    if !enabled(c)? {
        return Ok(());
    }
    c.execute("INSERT OR IGNORE INTO statistics_pending
        SELECT id FROM facts WHERE file_id IN (SELECT id FROM files WHERE path NOT IN (SELECT path FROM seen_files)) LIMIT ?1", [LIMIT+1])?;
    remember(c)
}

fn remember(c: &Connection) -> Result<()> {
    c.execute(
        "INSERT OR IGNORE INTO statistics_pending SELECT id FROM affected_facts LIMIT ?1",
        [LIMIT + 1],
    )?;
    c.execute_batch(
        "DELETE FROM statistics_pending WHERE id IN (SELECT id FROM statistics_previous);",
    )?;
    let count: i64 = c.query_row("SELECT (SELECT count(*) FROM statistics_previous)+(SELECT count(*) FROM statistics_pending)", [], |r| r.get(0))?;
    if count > LIMIT {
        return disable(c);
    }
    c.execute_batch("INSERT INTO statistics_previous
        SELECT p.id,COALESCE(SUM(r.resolution='resolved'),0),MAX(CASE WHEN r.resolution IN ('unresolved','ambiguous') THEN 1 ELSE 0 END)
        FROM statistics_pending p LEFT JOIN relationships r ON r.fact_id=p.id GROUP BY p.id;
        DELETE FROM statistics_pending;")?;
    Ok(())
}

pub(super) fn difference(c: &Connection) -> Result<Option<(i64, i64)>> {
    if !enabled(c)? {
        return Ok(None);
    }
    super::super::invalidation::include_dispatch(c)?;
    c.execute_batch(
        "INSERT OR IGNORE INTO statistics_pending SELECT id FROM statistics_previous;",
    )?;
    c.execute("INSERT OR IGNORE INTO statistics_pending SELECT id FROM facts WHERE file_id IN (SELECT id FROM changed_files) LIMIT ?1", [LIMIT+1])?;
    c.execute(
        "INSERT OR IGNORE INTO statistics_pending SELECT id FROM affected_facts LIMIT ?1",
        [LIMIT + 1],
    )?;
    let count: i64 = c.query_row("SELECT count(*) FROM statistics_pending", [], |r| r.get(0))?;
    if count > LIMIT {
        disable(c)?;
        return Ok(None);
    }
    let difference = c.query_row("SELECT COALESCE(SUM(current.resolved-COALESCE(prior.resolved,0)),0),
            COALESCE(SUM(current.bad-COALESCE(prior.bad,0)),0)
        FROM (SELECT p.id,COALESCE(SUM(r.resolution='resolved'),0) AS resolved,
            MAX(CASE WHEN r.resolution IN ('unresolved','ambiguous') THEN 1 ELSE 0 END) AS bad
            FROM statistics_pending p LEFT JOIN relationships r ON r.fact_id=p.id GROUP BY p.id) current
        LEFT JOIN statistics_previous prior ON prior.id=current.id", [], |r| Ok((r.get(0)?,r.get(1)?)))?;
    c.execute_batch("DELETE FROM statistics_pending; DELETE FROM statistics_previous;")?;
    Ok(Some(difference))
}

pub(super) fn apply(c: &Connection, generation: i64, resolved: i64, bad: i64) -> Result<bool> {
    let valid: bool = c.query_row(
        "SELECT relationships+?1>=0 AND unresolved+?2>=0 FROM coverage WHERE singleton=1",
        params![resolved, bad],
        |r| r.get(0),
    )?;
    if !valid {
        return Err(crate::domain::diagnostics::Error::operation(
            "incremental coverage would become negative",
        ));
    }
    Ok(c.execute("INSERT OR REPLACE INTO coverage
        SELECT 1,?1,input.scanned,input.analyzed,input.skipped,input.unsupported,input.bytes,
            (SELECT COUNT(DISTINCT key_blob) FROM symbols),previous.relationships+?2,previous.unresolved+?3
        FROM (SELECT COUNT(*) AS scanned,COALESCE(SUM(status='analyzed'),0) AS analyzed,
            COALESCE(SUM(status='skipped'),0) AS skipped,COALESCE(SUM(status='unsupported'),0) AS unsupported,
            COALESCE(SUM(CASE WHEN status='analyzed' THEN size ELSE 0 END),0) AS bytes FROM files) input
        CROSS JOIN coverage previous JOIN metadata m ON m.generation=previous.generation
        WHERE previous.singleton=1 AND m.singleton=1", params![generation,resolved,bad])? == 1)
}
