// Purpose: 갱신 전 이름·모듈 lookup과 대상 참조의 소비 fact를 디스크 집합으로 보존.
// Pattern: Storage Query.
// Usage: incremental 정책이 선언을 지우기 전에 호출하고 부분 resolver가 범위를 읽음.
// Related: crates/kodstar-core/src/application/analysis/incremental.rs, crates/kodstar-core/src/application/analysis/resolve/mod.rs.

use super::FileRecord;
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::path_bytes;
use rusqlite::{Connection, OptionalExtension};
pub(crate) mod static_modules;

pub(crate) fn safe_file(connection: &Connection, id: i64) -> Result<bool> {
    let language: String =
        connection.query_row("SELECT language FROM files WHERE id=?1", [id], |r| r.get(0))?;
    if language == "go" {
        package_functions(connection, id)
    } else if static_modules::group(&language).is_some() {
        static_modules::file(connection, id)
    } else {
        Ok(false)
    }
}

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

fn capture_keys(connection: &Connection, record: &FileRecord) -> Result<()> {
    if record.language == "go" {
        let prefix = format!("{}:{}:", record.package, record.module);
        // Record the provider selected by go.work/replace, including empty package lookups.
        let package =
            crate::domain::resolve::lookup_keys::go_package(&record.package, &record.module);
        connection.execute(
            "INSERT OR IGNORE INTO pending_lookup_keys
        SELECT id FROM lookup_keys k WHERE (k.kind='go_package' AND k.lookup_key=?2)
            OR (k.kind='module_name' AND substr(k.lookup_key,1,length(?1))=?1)",
            rusqlite::params![prefix, package],
        )?;
    } else {
        static_modules::capture(connection, record)?;
    }
    let key = blake3::hash(&path_bytes(&record.path)).to_hex().to_string();
    connection.execute(
        "INSERT OR IGNORE INTO pending_lookup_keys
        SELECT id FROM lookup_keys k WHERE k.kind='file' AND k.lookup_key=?1",
        [&key],
    )?;
    Ok(())
}

pub(crate) fn capture_imports(connection: &Connection, record: &FileRecord) -> Result<()> {
    capture_keys(connection, record)?;
    super::dependencies::affect_imports(connection)
}

pub(crate) fn capture(connection: &Connection, record: &FileRecord) -> Result<()> {
    capture_keys(connection, record)?;
    connection.execute(
        "INSERT OR IGNORE INTO affected_facts
        SELECT r.fact_id FROM relationships r
        JOIN symbols target ON target.id=r.target_id JOIN files f ON f.id=target.file_id
        WHERE f.path=?1",
        [path_bytes(&record.path)],
    )?;
    super::dependencies::affect(connection)?;
    include_dispatch(connection)?;
    connection.execute_batch("INSERT OR IGNORE INTO dependent_files SELECT f.file_id FROM affected_facts a JOIN facts f ON f.id=a.id;")?;
    Ok(())
}

pub(crate) fn capture_deleted(connection: &Connection) -> Result<bool> {
    let mut statement = connection.prepare(
        "SELECT id,path,language,module,package,content_hash,size,status FROM files
        WHERE id IN (SELECT id FROM deleted_files)",
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
        let language: String = row.get(2)?;
        let eligible = if language == "go" {
            only_go(connection)?
        } else {
            static_modules::project(connection, &language)?
        };
        if !eligible || !safe_file(connection, id)? {
            return Ok(false);
        }
        let record = FileRecord {
            path: crate::infrastructure::cache::paths::path_from_bytes(&path)?,
            language,
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
    connection.execute_batch("INSERT OR IGNORE INTO affected_facts
        SELECT id FROM facts WHERE file_id IN (SELECT id FROM changed_files)
        AND (file_id NOT IN (SELECT id FROM body_files)
            OR file_id IN (SELECT id FROM body_files WHERE granular=0) AND relation IN ('calls','references'));
        INSERT OR IGNORE INTO dependent_files SELECT file_id FROM facts WHERE id IN (SELECT id FROM affected_facts);
        INSERT OR IGNORE INTO dependent_files SELECT id FROM body_files;")?;
    include_dispatch(connection)?;
    super::statistics::delta::remember_selected(connection)?;
    super::relations::partial(connection)?;
    super::dependencies::prepare_selected(connection)?;
    connection.execute_batch(
        "
        DELETE FROM facts WHERE relation='dispatch_candidate'
            AND id IN (SELECT id FROM affected_facts);",
    )?;
    Ok(())
}

pub(crate) fn include_dispatch(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "INSERT OR IGNORE INTO affected_facts
        SELECT derived.id FROM affected_facts selected JOIN facts origin ON origin.id=selected.id
        JOIN facts derived ON derived.file_id=origin.file_id
            AND (derived.source_id=origin.source_id OR derived.source_id IN
                (SELECT canonical_id FROM symbol_aliases WHERE symbol_id=origin.source_id))
            AND derived.scope_id=origin.scope_id AND derived.start_byte=origin.start_byte
            AND derived.end_byte=origin.end_byte AND derived.expression=origin.expression
        WHERE origin.relation='calls' AND derived.relation='dispatch_candidate';",
    )?;
    Ok(())
}
