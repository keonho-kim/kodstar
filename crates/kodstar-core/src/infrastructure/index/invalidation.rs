// Purpose: 갱신 전 이름·모듈 lookup과 대상 참조의 소비 파일을 디스크 집합으로 보존.
// Pattern: Storage Query.
// Usage: incremental 정책이 선언을 지우기 전에 호출하고 부분 resolver가 범위를 읽음.
// Related: crates/kodstar-core/src/application/analysis/incremental.rs, crates/kodstar-core/src/application/analysis/resolve/mod.rs.

use super::FileRecord;
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::path_bytes;
use rusqlite::{Connection, OptionalExtension};

pub(crate) fn package_functions(connection: &Connection, id: i64) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM files WHERE id=?1 AND language='go' AND status='analyzed')
         AND NOT EXISTS(SELECT 1 FROM symbols WHERE file_id=?1 AND (language<>'go'
             OR kind NOT IN ('file','function') OR (kind='function' AND scope_id<>0)))
         AND NOT EXISTS(SELECT 1 FROM symbols fn JOIN files source ON source.id=fn.file_id
             JOIN symbols t ON t.name_key=fn.name_key JOIN files target ON target.id=t.file_id
             WHERE fn.file_id=?1 AND fn.kind='function' AND t.language='go'
               AND t.kind IN ('struct','type','interface','enum')
               AND source.package=target.package AND source.module=target.module)",
        [id],
        |r| r.get(0),
    )?)
}

pub(crate) fn prior_file(connection: &Connection, record: &FileRecord) -> Result<Option<i64>> {
    Ok(connection
        .query_row(
            "SELECT id FROM files WHERE path=?1",
            [path_bytes(&record.path)],
            |r| r.get(0),
        )
        .optional()?)
}

pub(crate) fn only_go(connection: &Connection) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM files WHERE status='analyzed' AND language<>'go')
         AND NOT EXISTS(SELECT 1 FROM facts WHERE language<>'go')",
        [],
        |r| r.get(0),
    )?)
}

pub(crate) fn capture(connection: &Connection, record: &FileRecord) -> Result<()> {
    let prefix = format!("{}:{}:", record.package, record.module);
    // Imported namespaces may be mapped through go.work or replace. Observe every Go module
    // consumer conservatively; same-package names can use the exact package/module prefix.
    connection.execute(
        "INSERT OR IGNORE INTO dependent_files
        SELECT b.file_id FROM lookup_keys k CROSS JOIN lookup_blocks b ON b.key_id=k.id
        JOIN files u ON u.id=b.file_id
        WHERE u.language='go' AND (k.kind='module'
            OR (k.kind='module_name' AND substr(k.lookup_key,1,length(?1))=?1))",
        [&prefix],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO dependent_files
        SELECT u.file_id FROM relationships r JOIN facts u ON u.id=r.fact_id
        JOIN symbols target ON target.id=r.target_id JOIN files f ON f.id=target.file_id
        WHERE f.path=?1",
        [path_bytes(&record.path)],
    )?;
    let key = blake3::hash(&path_bytes(&record.path)).to_hex().to_string();
    connection.execute(
        "INSERT OR IGNORE INTO dependent_files
        SELECT b.file_id FROM lookup_keys k CROSS JOIN lookup_blocks b ON b.key_id=k.id
        WHERE k.kind='file' AND k.lookup_key=?1",
        [&key],
    )?;
    Ok(())
}

pub(crate) fn capture_deleted(connection: &Connection) -> Result<bool> {
    if !only_go(connection)? {
        return Ok(false);
    }
    let mut statement = connection.prepare(
        "SELECT id,path,language,module,package,lower(hex(content_hash)),size,status FROM files
        WHERE path NOT IN (SELECT path FROM seen_files)",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id = row.get(0)?;
        let path: Vec<u8> = row.get(1)?;
        let status: String = row.get(7)?;
        if matches!(status.as_str(), "unsupported" | "skipped") {
            if super::inert_inputs::detached(connection, id, &path)? {
                continue;
            }
            return Ok(false);
        }
        if !package_functions(connection, id)? {
            return Ok(false);
        }
        let record = FileRecord {
            path: crate::infrastructure::cache::paths::path_from_bytes(&path)?,
            language: row.get(2)?,
            module: row.get(3)?,
            package: row.get(4)?,
            content_hash: row.get(5)?,
            size: row.get(6)?,
            status: "analyzed",
        };
        capture(connection, &record)?;
    }
    Ok(true)
}

pub(crate) fn prepare_partial(connection: &Connection) -> Result<()> {
    connection
        .execute_batch("INSERT OR IGNORE INTO changed_files SELECT id FROM dependent_files;")?;
    super::relations::partial(connection)?;
    connection.execute_batch("
        INSERT OR IGNORE INTO symbol_identities SELECT id,id FROM symbols WHERE language='go';
        DELETE FROM facts WHERE relation='dispatch_candidate' AND file_id IN (SELECT id FROM changed_files);
        DELETE FROM lookup_blocks WHERE file_id IN (SELECT id FROM changed_files);")?;
    Ok(())
}
