// Purpose: 새 의존성과 선택된 이전 소비를 제한된 블록으로 대조·공개.
// Pattern: Incremental Merge.
// Usage: resolver publication이 같은 분석 transaction에서 호출.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/selection.rs, crates/kodstar-core/src/infrastructure/index/dependencies/writer.rs.

use super::{codec, selection, writer::Writer};
use crate::domain::diagnostics::Result;
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn publish(c: &Connection) -> Result<()> {
    selection::initialize(c)?;
    super::super::dictionary::publish(c)?;
    let empty: bool = c.query_row("SELECT NOT EXISTS(SELECT 1 FROM lookup_rows)", [], |r| {
        r.get(0)
    })?;
    if empty {
        fresh(c)?;
        c.execute_batch(
            "DROP TABLE dictionary_pending; DELETE FROM dependency_groups;
            UPDATE dependency_selection SET partial=0;",
        )?;
        return Ok(());
    }
    c.execute_batch("CREATE TEMP TABLE dependency_rows(key_id INTEGER,file_id INTEGER,fact_id INTEGER,found INTEGER,
        PRIMARY KEY(key_id,file_id,fact_id)) WITHOUT ROWID;
        INSERT INTO dependency_rows SELECT k.key_id,f.file_id,d.fact_id,d.found
        FROM resolution_dependencies d JOIN dictionary_pending k ON k.kind=d.kind AND k.lookup_key=d.lookup_key
        JOIN facts f ON f.id=d.fact_id;
        INSERT OR IGNORE INTO dependency_groups SELECT DISTINCT key_id,file_id FROM dependency_rows;")?;
    let partial = c.query_row("SELECT partial FROM dependency_selection", [], |r| r.get(0))?;
    let mut writer = Writer::new(c, partial)?;
    let mut groups = c.prepare(
        "SELECT key_id,file_id FROM dependency_groups
        WHERE (key_id,file_id)>(?1,?2) ORDER BY key_id,file_id LIMIT 1",
    )?;
    let mut after = (0i64, 0i64);
    loop {
        let next: Option<(i64, i64)> = groups
            .query_row(params![after.0, after.1], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let Some((key, file)) = next else { break };
        writer.merge(key, file)?;
        after = (key, file);
    }
    drop(groups);
    drop(writer);
    c.execute_batch(
        "DROP TABLE dependency_rows; DROP TABLE dictionary_pending;
        DELETE FROM dependency_groups; UPDATE dependency_selection SET partial=0;",
    )?;
    Ok(())
}

fn fresh(c: &Connection) -> Result<()> {
    let mut select = c.prepare("SELECT k.key_id,f.file_id,d.fact_id,d.found
        FROM resolution_dependencies d JOIN dictionary_pending k ON k.kind=d.kind AND k.lookup_key=d.lookup_key
        JOIN facts f ON f.id=d.fact_id ORDER BY k.key_id,f.file_id,d.fact_id")?;
    let mut rows = select.query([])?;
    let mut insert = c.prepare(
        "INSERT INTO lookup_rows(key_id,file_id,part,count,payload) VALUES(?1,?2,?3,?4,?5)",
    )?;
    let (mut group, mut part, mut count, mut previous) = (None, 0, 0, 0);
    let mut bytes = Vec::with_capacity(codec::MAX_COUNT * 2);
    while let Some(row) = rows.next()? {
        let current = (row.get::<_, i64>(0)?, row.get::<_, i64>(1)?);
        if group != Some(current) || count == codec::MAX_COUNT {
            if let Some((key, file)) = group {
                insert.execute(params![key, file, part, count, bytes])?;
            }
            part = if group == Some(current) { part + 1 } else { 0 };
            group = Some(current);
            count = 0;
            previous = 0;
            bytes.clear();
        }
        let fact = row.get(2)?;
        codec::push(&mut bytes, fact, previous, row.get(3)?)?;
        previous = fact;
        count += 1;
    }
    if let Some((key, file)) = group {
        insert.execute(params![key, file, part, count, bytes])?;
    }
    Ok(())
}
