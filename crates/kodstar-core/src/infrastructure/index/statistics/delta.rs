// Purpose: 작은 증분 변경의 이전·이후 fact 통계 차이를 같은 transaction에서 계산.
// Pattern: Bounded Journal.
// Usage: 분석 writer가 삭제 전에 기록하고 통계 공개가 새 파생 fact까지 대조.
// Related: crates/kodstar-core/src/infrastructure/index/statistics.rs, crates/kodstar-core/src/application/analysis/pipeline.rs.

use crate::domain::diagnostics::Result;
use rusqlite::{Connection, params};

#[cfg(test)]
mod tests;

pub(crate) fn reset(c: &Connection, eligible: bool, budget: usize) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS statistics_delta(enabled INTEGER NOT NULL,limit_count INTEGER NOT NULL);
        CREATE TEMP TABLE IF NOT EXISTS statistics_previous(id INTEGER PRIMARY KEY,resolved INTEGER,bad INTEGER);
        CREATE TEMP TABLE IF NOT EXISTS statistics_pending(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE IF NOT EXISTS body_files(id INTEGER PRIMARY KEY,granular INTEGER NOT NULL DEFAULT 0);
        DELETE FROM statistics_delta; DELETE FROM statistics_previous; DELETE FROM statistics_pending;")?;
    let (large, limit) = if eligible {
        let prior = super::read(c)?;
        let size = prior.relationships.saturating_add(prior.unresolved);
        (
            size >= 512,
            (size / 4).clamp(128, budget.clamp(128, 16384) as u64),
        )
    } else {
        (false, 128)
    };
    c.execute(
        "INSERT INTO statistics_delta VALUES(?1,?2)",
        params![large, limit],
    )?;
    Ok(())
}

fn limit(c: &Connection) -> Result<i64> {
    Ok(c.query_row("SELECT limit_count FROM statistics_delta", [], |r| r.get(0))?)
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
        params![file, limit(c)? + 1],
    )?;
    remember(c)
}

pub(crate) fn remember_deleted(c: &Connection) -> Result<()> {
    if !enabled(c)? {
        return Ok(());
    }
    c.execute(
        "INSERT OR IGNORE INTO statistics_pending
        SELECT id FROM facts WHERE file_id IN (SELECT id FROM deleted_files) LIMIT ?1",
        [limit(c)? + 1],
    )?;
    remember(c)
}

pub(crate) fn remember_selected(c: &Connection) -> Result<()> {
    if enabled(c)? {
        remember(c)?;
    }
    Ok(())
}

fn remember(c: &Connection) -> Result<()> {
    c.execute(
        "INSERT OR IGNORE INTO statistics_pending SELECT id FROM affected_facts LIMIT ?1",
        [limit(c)? + 1],
    )?;
    c.execute_batch(
        "DELETE FROM statistics_pending WHERE id IN (SELECT id FROM statistics_previous);",
    )?;
    let count: i64 = c.query_row("SELECT (SELECT count(*) FROM statistics_previous)+(SELECT count(*) FROM statistics_pending)", [], |r| r.get(0))?;
    if count > limit(c)? {
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
    c.execute("INSERT OR IGNORE INTO statistics_pending SELECT id FROM facts WHERE file_id IN (SELECT id FROM changed_files) AND file_id NOT IN (SELECT id FROM body_files WHERE granular=1) LIMIT ?1", [limit(c)?+1])?;
    c.execute(
        "INSERT OR IGNORE INTO statistics_pending SELECT id FROM affected_facts LIMIT ?1",
        [limit(c)? + 1],
    )?;
    let count: i64 = c.query_row("SELECT count(*) FROM statistics_pending", [], |r| r.get(0))?;
    if count > limit(c)? {
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
