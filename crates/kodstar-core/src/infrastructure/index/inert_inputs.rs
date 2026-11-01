// Purpose: 사실을 제공하지 않는 기존 입력의 이력 전용 수정·삭제를 판정.
// Pattern: Storage Query.
// Usage: 분석 pipeline과 삭제 조정자가 기존 행을 지우기 전에 확인.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/infrastructure/index/write.rs.

use super::FileRecord;
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::path_bytes;
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn detached(connection: &Connection, id: i64, path: &[u8]) -> Result<bool> {
    let key = blake3::hash(path).to_hex().to_string();
    Ok(connection.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM symbols WHERE file_id=?1)
         AND NOT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1)
         AND NOT EXISTS(SELECT 1 FROM facts WHERE file_id=?1)
         AND NOT EXISTS(SELECT 1 FROM lookup_keys k CROSS JOIN lookup_blocks b ON b.key_id=k.id WHERE k.kind='file' AND k.lookup_key=?2)",
        params![id, key],
        |r| r.get(0),
    )?)
}

pub(crate) fn unchanged_kind(connection: &Connection, record: &FileRecord) -> Result<bool> {
    let path = path_bytes(&record.path);
    let id: Option<i64> = connection
        .query_row(
            "SELECT id FROM files WHERE path=?1 AND status=?2 AND language=?3
         AND path_module=?4 AND path_package=?5 AND status IN ('unsupported','skipped')",
            params![
                &path,
                record.status,
                record.language,
                record.module,
                record.package
            ],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => detached(connection, id, &path),
        None => Ok(false),
    }
}

pub(crate) fn semantic_deletions(connection: &Connection) -> Result<bool> {
    let mut statement = connection
        .prepare("SELECT id,path,status FROM files WHERE id IN (SELECT id FROM deleted_files)")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let status: String = row.get(2)?;
        if !matches!(status.as_str(), "unsupported" | "skipped")
            || !detached(connection, row.get(0)?, &row.get::<_, Vec<u8>>(1)?)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}
