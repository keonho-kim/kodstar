// Purpose: 추적 상태의 변경 판정과 체크포인트 공개 정책.
// Pattern: Use Case.
// Usage: 공통 동기화와 이력 명령이 저장소 잠금 아래 호출.
// Related: crates/kodstar-core/src/infrastructure/history/content/blobs.rs, crates/kodstar-core/src/infrastructure/scan/mod.rs.

use crate::application::workspace::Settings;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    history::HistoryState,
};
use crate::infrastructure::history::{Repository, checkpoints, content::blobs, paths::file_mode};
use crate::infrastructure::{
    cache::{now_seconds, paths::path_bytes},
    scan,
};
use rusqlite::params;
use std::fs::{self, File};
use std::path::Path;

mod catalog;
mod seen;

pub(crate) fn run(
    repository: &mut Repository,
    cache_home: &Path,
    settings: &Settings,
    verify: bool,
    cancel: &Cancellation,
) -> Result<HistoryState> {
    let _span = tracing::info_span!("capture", verify).entered();
    let connection = &mut repository.connection;
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute_batch("CREATE TEMP TABLE IF NOT EXISTS observed AS SELECT * FROM working WHERE 0; DELETE FROM observed;")?;
    transaction
        .execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS temp.observed_path ON observed(path);")?;
    let mut contents = blobs::Writer::new(&transaction)?;
    let mut catalog = catalog::Catalog::new(&transaction)?;
    let previous_count =
        transaction.query_row("SELECT count(*) FROM working", [], |row| row.get(0))?;
    let mut seen = seen::Seen::new(settings.memory_budget / 64);
    let mut tracked = 0u64;
    scan::walk(
        &repository.root,
        cache_home,
        &settings.exclude,
        cancel,
        &mut |entry| {
            cancel.check()?;
            let absolute = &entry.absolute;
            let metadata = &entry.metadata;
            let path = path_bytes(&entry.relative);
            let (modified, changed) = scan::metadata_key(metadata)?;
            let kind = if metadata.file_type().is_symlink() {
                "symlink"
            } else {
                "file"
            };
            let mode = file_mode(metadata);
            let old = catalog.previous(&path)?;
            let matching_kind = old.as_ref().filter(|value| value.kind == kind);
            let previous = matching_kind.map(|value| value.blob);
            tracked += 1;
            if !verify
                && matching_kind.is_some_and(|value| {
                    value.size == metadata.len()
                        && value.modified == modified
                        && value.changed == changed
                        && value.mode == mode
                })
            {
                return seen.push(&transaction, path, cancel);
            }
            let blob = {
                crate::infrastructure::history::paths::destination(
                    &repository.root,
                    &entry.relative,
                )?;
                let before = fs::symlink_metadata(absolute)
                    .map_err(|e| Error::io("inspect history input", absolute, e))?;
                if before.file_type() != metadata.file_type()
                    || before.len() != metadata.len()
                    || scan::metadata_key(&before)? != scan::metadata_key(metadata)?
                    || file_mode(&before) != mode
                {
                    return Err(Error::operation(
                        "history input changed after discovery; checkpoint was not published",
                    ));
                }
                let blob = if kind == "symlink" {
                    let target = fs::read_link(absolute)
                        .map_err(|e| Error::io("read history symlink", absolute, e))?;
                    let target = path_bytes(&target);
                    let size = target.len() as u64;
                    contents.put(std::io::Cursor::new(target), size, previous, cancel)?
                } else {
                    let file = File::open(absolute)
                        .map_err(|e| Error::io("open history input", absolute, e))?;
                    let opened = file
                        .metadata()
                        .map_err(|e| Error::io("inspect opened history input", absolute, e))?;
                    if !opened.is_file()
                        || scan::metadata_key(&opened)? != scan::metadata_key(&before)?
                    {
                        return Err(Error::operation("history input changed before capture"));
                    }
                    contents.put(file, metadata.len(), previous, cancel)?
                };
                let after = fs::symlink_metadata(absolute)
                    .map_err(|e| Error::io("recheck history input", absolute, e))?;
                if before.file_type() != after.file_type()
                    || before.len() != after.len()
                    || scan::metadata_key(&before)? != scan::metadata_key(&after)?
                    || file_mode(&before) != file_mode(&after)
                {
                    return Err(Error::operation(
                        "history input changed during capture; checkpoint was not published",
                    ));
                }
                blob
            };
            catalog.observe(params![
                &path,
                blob,
                mode,
                kind,
                metadata.len(),
                modified,
                changed
            ])?;
            if old.is_some() {
                seen.push(&transaction, path, cancel)?;
            }
            Ok(())
        },
    )?;
    drop(contents);
    drop(catalog);
    let parent = checkpoints::head(&transaction)?;
    let deleted = seen.deleted(&transaction, previous_count, cancel)?;
    let updated: u64 = transaction.query_row("SELECT count(*) FROM observed o LEFT JOIN working w ON w.path=o.path WHERE w.path IS NULL OR o.blob<>w.blob OR o.mode<>w.mode OR o.kind<>w.kind", [], |r| r.get(0))?;
    let count = updated + deleted;
    let created = count > 0 || parent.is_none();
    if created {
        transaction.execute(
            "INSERT INTO checkpoints(parent,created_at,changed_files,tracked_files) VALUES(?1,?2,?3,?4)",
            params![parent, now_seconds()?, count, tracked],
        )?;
        let id = transaction.last_insert_rowid();
        transaction.execute("INSERT INTO entries SELECT ?1,o.path,o.blob,o.mode,o.kind FROM observed o LEFT JOIN working w ON w.path=o.path WHERE w.path IS NULL OR o.blob<>w.blob OR o.mode<>w.mode OR o.kind<>w.kind", [id])?;
        if deleted != 0 {
            transaction.execute("INSERT INTO entries SELECT ?1,w.path,NULL,w.mode,w.kind FROM deleted_paths d CROSS JOIN working w ON w.path=d.path", [id])?;
        }
        transaction.execute(
            "UPDATE branches SET checkpoint=?1 WHERE name=(SELECT current_branch FROM repository)",
            [id],
        )?;
    }
    if deleted != 0 {
        transaction.execute_batch(
            "DELETE FROM working WHERE path IN (SELECT path FROM deleted_paths);
                                   DROP TABLE deleted_paths;",
        )?;
    }
    transaction.execute_batch("INSERT INTO working SELECT * FROM observed o WHERE NOT EXISTS(SELECT 1 FROM working w WHERE w.path=o.path AND w.blob=o.blob AND w.mode=o.mode AND w.kind=o.kind AND w.size=o.size AND w.modified=o.modified AND w.changed=o.changed)
        ON CONFLICT(path) DO UPDATE SET blob=excluded.blob,mode=excluded.mode,kind=excluded.kind,size=excluded.size,modified=excluded.modified,changed=excluded.changed;")?;
    cancel.check()?;
    transaction.commit()?;
    let mut state = repository.state()?;
    state.created = created;
    state.changed_files = count;
    Ok(state)
}
