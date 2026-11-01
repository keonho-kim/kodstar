// Purpose: 이전 소비 의존성을 유지한 채 재해석 후 대조할 key·파일 그룹 선택.
// Pattern: Deferred Mutation.
// Usage: 부분 resolver가 changed_files·affected_facts를 정한 뒤 호출.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/publication.rs.

use super::codec;
use crate::domain::diagnostics::Result;
use rusqlite::{Connection, params, params_from_iter};

pub(super) fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS dependency_groups(
        key_id INTEGER,file_id INTEGER,PRIMARY KEY(key_id,file_id)) WITHOUT ROWID;
        CREATE TEMP TABLE IF NOT EXISTS dependency_selection(partial INTEGER);
        CREATE TEMP TABLE IF NOT EXISTS body_files(id INTEGER PRIMARY KEY);
        INSERT INTO dependency_selection SELECT 0 WHERE NOT EXISTS(SELECT 1 FROM dependency_selection);")?;
    Ok(())
}

pub(crate) fn prepare_selected(c: &Connection) -> Result<()> {
    initialize(c)?;
    c.execute_batch(
        "DELETE FROM dependency_groups; UPDATE dependency_selection SET partial=1;
        INSERT INTO dependency_groups SELECT DISTINCT key_id,file_id FROM lookup_rows
            WHERE file_id IN (SELECT id FROM changed_files) AND file_id NOT IN (SELECT id FROM body_files);",
    )?;
    let slots = std::iter::repeat_n("?", codec::MAX_COUNT)
        .collect::<Vec<_>>()
        .join(",");
    let mut selected = c.prepare(&format!(
        "SELECT EXISTS(SELECT 1 FROM affected_facts WHERE id IN ({slots}))"
    ))?;
    let mut blocks = c.prepare(
        "SELECT key_id,file_id,count,payload FROM lookup_rows
        WHERE file_id IN (SELECT id FROM dependent_files)
            AND file_id NOT IN (SELECT id FROM changed_files WHERE id NOT IN (SELECT id FROM body_files))",
    )?;
    let mut rows = blocks.query([])?;
    let mut remember = c.prepare("INSERT OR IGNORE INTO dependency_groups VALUES(?1,?2)")?;
    while let Some(row) = rows.next()? {
        let mut ids = Vec::with_capacity(codec::MAX_COUNT);
        codec::visit(
            row.get_ref(3)?
                .as_blob()
                .map_err(|e| crate::domain::diagnostics::Error::caused_by("lookup payload", e))?,
            row.get(2)?,
            |id, _| {
                ids.push(Some(id));
                Ok(())
            },
        )?;
        ids.resize(codec::MAX_COUNT, None);
        if selected.query_row(params_from_iter(ids), |r| r.get::<_, bool>(0))? {
            remember.execute(params![row.get::<_, i64>(0)?, row.get::<_, i64>(1)?])?;
        }
    }
    Ok(())
}
