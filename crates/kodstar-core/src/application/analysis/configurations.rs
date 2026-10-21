// Purpose: 명시적으로 참조한 설정을 인덱스 내부에서 제한적으로 추가 분석.
// Pattern: Coordinator.
// Usage: 일반 파일 분석과 삭제 반영 후 같은 갱신 transaction에서 호출.
// Related: crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/src/application/analysis/resolve/indexed_paths.rs.

use super::{module_identity::module_identity, resolve::indexed_paths};
use crate::application::workspace::SelectedProject;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::languages::Language;
use crate::domain::results::Progress;
use crate::infrastructure::cache::paths::path_from_bytes;
use crate::infrastructure::parse::ParseSession;
use crate::infrastructure::{formats, index, scan};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub(super) fn run(
    selected: &SelectedProject,
    connection: &Connection,
    generation: i64,
    cancel: &Cancellation,
    progress: &mut dyn FnMut(&Progress) -> Result<()>,
) -> Result<(bool, u64)> {
    connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS configuration_queue(id INTEGER PRIMARY KEY,depth INTEGER,done INTEGER); DELETE FROM configuration_queue;
        INSERT OR IGNORE INTO configuration_queue SELECT file_id,0,0 FROM bindings WHERE kind='configuration' AND name='root';")?;
    let mut parser = None;
    let mut parsed = 0;
    loop {
        cancel.check()?;
        let next: Option<(i64, usize)> = connection
            .query_row(
                "SELECT id,depth FROM configuration_queue WHERE done=0 ORDER BY depth,id LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((id, depth)) = next else { break };
        if depth > 32 {
            return Err(Error::new(
                ErrorKind::Limit,
                "configuration references exceed 32 levels",
            ));
        }
        connection.execute("UPDATE configuration_queue SET done=1 WHERE id=?1", [id])?;
        let known: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND kind='configuration')",
            [id],
            |r| r.get(0),
        )?;
        let raw_path: Vec<u8> =
            connection.query_row("SELECT path FROM files WHERE id=?1", [id], |r| r.get(0))?;
        let path = path_from_bytes(&raw_path)?;
        if !known {
            parse(
                selected,
                connection,
                generation,
                id,
                &path,
                parser.get_or_insert_with(|| ParseSession::new(selected.settings.fact_batch_bytes)),
                cancel,
            )?;
            parsed += 1;
            progress(&Progress {
                phase: "configuration",
                completed_files: parsed,
                source_bytes: 0,
            })?;
        }
        let mut links=connection.prepare("SELECT name,module_name FROM bindings WHERE file_id=?1 AND kind='configuration_link' ORDER BY start_byte,id")?;
        let links = links.query_map([id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for link in links {
            let (kind, raw) = link?;
            if raw.starts_with('@') {
                external(connection, id)?;
                continue;
            }
            let target = indexed_paths::configuration_file(
                connection,
                path.parent().unwrap_or_else(|| Path::new("")),
                &raw,
                &kind,
            )?;
            if let Some(target) = target {
                connection.execute(
                    "INSERT OR IGNORE INTO configuration_queue VALUES(?1,?2,0)",
                    params![target, depth + 1],
                )?;
            } else if raw.starts_with('.') || raw.starts_with('/') {
                return Err(Error::operation(format!(
                    "referenced configuration is outside the indexed inputs or missing: {}",
                    path.display()
                )));
            } else {
                external(connection, id)?;
            }
        }
        let count: u64 =
            connection.query_row("SELECT COUNT(*) FROM configuration_queue", [], |r| r.get(0))?;
        if count > 4096 {
            return Err(Error::new(
                ErrorKind::Limit,
                "configuration references exceed 4096 files",
            ));
        }
    }
    connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS configuration_orphans(id INTEGER PRIMARY KEY); DELETE FROM configuration_orphans;
        INSERT INTO configuration_orphans SELECT f.id FROM files f JOIN bindings b ON b.file_id=f.id WHERE b.kind='configuration' AND b.name='referenced' AND f.id NOT IN (SELECT id FROM configuration_queue);")?;
    let mut changed = parsed > 0;
    loop {
        cancel.check()?;
        let orphan:Option<(i64,Vec<u8>)>=connection.query_row("SELECT f.id,f.path FROM files f JOIN configuration_orphans o ON o.id=f.id ORDER BY f.id LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((id, path)) = orphan else { break };
        connection.execute("DELETE FROM configuration_orphans WHERE id=?1", [id])?;
        let path = path_from_bytes(&path)?;
        let (package, module) = module_identity(&selected.root, &path, None);
        index::clear_file(connection, id)?;
        connection.execute("UPDATE files SET status='unsupported',language='unsupported',package=?2,module=?3,path_module=?3,content_hash='' WHERE id=?1",params![id,package,module])?;
        connection.execute("INSERT INTO diagnostics(file_id,code,message) VALUES(?1,'unsupported_input','unsupported_input')",[id])?;
        changed = true;
    }
    Ok((changed, parsed))
}

fn parse(
    selected: &SelectedProject,
    connection: &Connection,
    generation: i64,
    id: i64,
    path: &Path,
    parser: &mut ParseSession,
    cancel: &Cancellation,
) -> Result<()> {
    let status: String =
        connection.query_row("SELECT status FROM files WHERE id=?1", [id], |r| r.get(0))?;
    if status == "skipped" {
        return Err(Error::new(
            ErrorKind::Limit,
            "referenced configuration exceeds the input limit",
        ));
    }
    if status != "unsupported" {
        return Err(Error::operation(
            "referenced configuration is already classified as a different input kind",
        ));
    }
    let absolute = selected.root.join(path);
    let canonical = std::fs::canonicalize(&absolute)
        .map_err(|e| Error::io("resolve configuration", &absolute, e))?;
    let metadata = std::fs::symlink_metadata(&absolute)
        .map_err(|e| Error::io("inspect configuration", &absolute, e))?;
    if !canonical.starts_with(&selected.root) || !metadata.is_file() {
        return Err(Error::operation(
            "configuration is outside the indexed root or no longer a regular file",
        ));
    }
    let entry = scan::Entry {
        relative: path.to_path_buf(),
        absolute,
        metadata,
        language: None,
        auxiliary: true,
        detect_prefix: false,
    };
    let (source, metadata) = scan::read_stable(&entry, selected.settings.max_file_bytes)?;
    if source.len() > selected.settings.max_file_bytes {
        return Err(Error::new(
            ErrorKind::Limit,
            "referenced configuration exceeds max_file_bytes",
        ));
    }
    let after = std::fs::canonicalize(&entry.absolute)
        .map_err(|e| Error::io("recheck configuration", &entry.absolute, e))?;
    if !after.starts_with(&selected.root) {
        return Err(Error::operation(
            "configuration target changed outside the indexed root",
        ));
    }
    let (package, module) = module_identity(&selected.root, path, Some(Language::TypeScript));
    let (modified_ns, changed_ns) = scan::metadata_key(&metadata)?;
    let record = index::FileRecord {
        path: path.to_path_buf(),
        language: "auxiliary".into(),
        module,
        package,
        content_hash: blake3::hash(&source).to_hex().to_string(),
        size: metadata.len(),
        modified_ns,
        changed_ns,
        status: "auxiliary",
    };
    record.persist(connection, generation)?;
    index::clear_file(connection, id)?;
    let mut context = index::FileFacts::new(id, &record, false);
    let mut writer = index::FactWriter::new(connection)?;
    formats::extract_configuration(parser, path, &source, cancel, &mut |language, fact| {
        cancel.check()?;
        if fact.owned_bytes() > selected.settings.fact_batch_bytes {
            return Err(Error::new(
                ErrorKind::Limit,
                "configuration fact exceeds fact_batch_bytes",
            ));
        }
        writer.persist(&mut context, language, fact)
    })
}

fn external(connection: &Connection, file: i64) -> Result<()> {
    connection.execute("INSERT INTO diagnostics(file_id,code,message) SELECT ?1,'external_configuration','external configuration is not indexed; no package was downloaded' WHERE NOT EXISTS(SELECT 1 FROM diagnostics WHERE file_id=?1 AND code='external_configuration')",[file])?;
    Ok(())
}
