// Purpose: 같은 선언 환경의 호출 fact를 제한된 digest·ID journal로 재사용.
// Pattern: Bounded Reconciliation.
// Usage: 파일 writer가 prepare·match·finish하고 분석이 임시 journal을 회수.
// Related: crates/kodstar-core/src/infrastructure/index/body_reuse/matcher.rs, crates/kodstar-core/src/application/analysis/pipeline/start.rs.

mod matcher;
use crate::domain::diagnostics::Result;
pub(crate) use matcher::Matcher;
use rusqlite::{Connection, Row, params};

const FIELDS: &str = "f.file_id,f.source_id,f.scope_id,f.expression,f.relation,coalesce(f.condition,0),
    f.type_only,f.start_byte,f.end_byte,f.line,f.column_byte,f.end_line,f.end_column_byte,f.language";

pub(crate) fn prepare(c: &Connection, file: i64, budget: usize) -> Result<bool> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS body_original(
        file_id INTEGER,digest BLOB,fact_id INTEGER,PRIMARY KEY(file_id,digest,fact_id)) WITHOUT ROWID;")?;
    let limit = budget / 128;
    let count: usize = c.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM facts_rows WHERE file_id=?1
        AND relation IN (SELECT id FROM atoms WHERE value IN ('calls','references')) LIMIT ?2)",
        params![file, limit.saturating_add(1)],
        |r| r.get(0),
    )?;
    if count > limit {
        return Ok(false);
    }
    let mut select = c.prepare(&format!(
        "SELECT f.id,{FIELDS} FROM facts_rows f WHERE f.file_id=?1
        AND f.relation IN (SELECT id FROM atoms WHERE value IN ('calls','references'))"
    ))?;
    let mut rows = select.query([file])?;
    let mut insert = c.prepare("INSERT INTO body_original VALUES(?1,?2,?3)")?;
    while let Some(row) = rows.next()? {
        insert.execute(params![
            file,
            digest(&fields(row, 1)?).as_bytes(),
            row.get::<_, i64>(0)?
        ])?;
    }
    Ok(true)
}

pub(crate) fn finish(c: &Connection, file: i64) -> Result<()> {
    let pending: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM body_original WHERE file_id=?1)",
        [file],
        |r| r.get(0),
    )?;
    if !pending {
        return super::statistics::delta::remember_selected(c);
    }
    c.execute(
        "INSERT OR IGNORE INTO affected_facts SELECT fact_id FROM body_original WHERE file_id=?1",
        [file],
    )?;
    super::invalidation::include_dispatch(c)?;
    super::statistics::delta::remember_selected(c)?;
    super::relations::clear_bodies(c, file, true)?;
    c.execute(
        "DELETE FROM facts WHERE id IN (SELECT fact_id FROM body_original WHERE file_id=?1)",
        [file],
    )?;
    c.execute("DELETE FROM body_original WHERE file_id=?1", [file])?;
    Ok(())
}

pub(crate) fn clear(c: &Connection) -> Result<()> {
    c.execute_batch("DROP TABLE IF EXISTS temp.body_original;")?;
    Ok(())
}

fn fields(row: &Row<'_>, start: usize) -> rusqlite::Result<[i64; 14]> {
    let mut values = [0; 14];
    for (offset, value) in values.iter_mut().enumerate() {
        *value = row.get(start + offset)?;
    }
    Ok(values)
}

fn digest(values: &[i64; 14]) -> blake3::Hash {
    let mut hash = blake3::Hasher::new();
    hash.update(b"kodstar-body-fact-v1");
    for value in values {
        hash.update(&value.to_le_bytes());
    }
    hash.finalize()
}
