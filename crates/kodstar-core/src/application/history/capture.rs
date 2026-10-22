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
use rusqlite::{OptionalExtension, params};
use std::fs::{self, File};
use std::path::Path;

pub(crate) fn run(
    repository: &mut Repository,
    cache_home: &Path,
    settings: &Settings,
    verify: bool,
    cancel: &Cancellation,
) -> Result<HistoryState> {
    let connection = &mut repository.connection;
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute_batch("CREATE TEMP TABLE IF NOT EXISTS observed AS SELECT * FROM working WHERE 0; DELETE FROM observed;")?;
    transaction
        .execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS temp.observed_path ON observed(path);")?;
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
            let old: Option<(i64, u64, String, String, u32)> = transaction
                .query_row(
                    "SELECT blob,size,modified,changed,mode FROM working WHERE path=?1 AND kind=?2",
                    params![path, kind],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()?;
            let previous = old.as_ref().map(|v| v.0);
            let blob = if !verify
                && old.as_ref().is_some_and(|(_, size, m, c, permissions)| {
                    *size == metadata.len()
                        && m == &modified
                        && c == &changed
                        && *permissions == mode
                }) {
                old.map(|v| v.0)
                    .ok_or_else(|| Error::operation("missing previous history input"))?
            } else {
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
                    blobs::put(
                        &transaction,
                        std::io::Cursor::new(path_bytes(&target)),
                        previous,
                        cancel,
                    )?
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
                    blobs::put(&transaction, file, previous, cancel)?
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
            transaction.execute(
                "INSERT INTO observed VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![path, blob, mode, kind, metadata.len(), modified, changed],
            )?;
            Ok(())
        },
    )?;
    let parent = checkpoints::head(&transaction)?;
    let count: u64 = transaction.query_row("SELECT (SELECT count(*) FROM observed o LEFT JOIN working w ON w.path=o.path WHERE w.path IS NULL OR o.blob<>w.blob OR o.mode<>w.mode OR o.kind<>w.kind) + (SELECT count(*) FROM working w WHERE NOT EXISTS(SELECT 1 FROM observed o WHERE o.path=w.path))", [], |r| r.get(0))?;
    let created = count > 0 || parent.is_none();
    if created {
        transaction.execute(
            "INSERT INTO checkpoints(parent,created_at,changed_files,tracked_files) VALUES(?1,?2,?3,(SELECT count(*) FROM observed))",
            params![parent, now_seconds()?, count],
        )?;
        let id = transaction.last_insert_rowid();
        transaction.execute("INSERT INTO entries SELECT ?1,o.path,o.blob,o.mode,o.kind FROM observed o LEFT JOIN working w ON w.path=o.path WHERE w.path IS NULL OR o.blob<>w.blob OR o.mode<>w.mode OR o.kind<>w.kind", [id])?;
        transaction.execute("INSERT INTO entries SELECT ?1,w.path,NULL,w.mode,w.kind FROM working w WHERE NOT EXISTS(SELECT 1 FROM observed o WHERE o.path=w.path)", [id])?;
        transaction.execute(
            "UPDATE branches SET checkpoint=?1 WHERE name=(SELECT current_branch FROM repository)",
            [id],
        )?;
    }
    transaction.execute_batch("DELETE FROM working WHERE NOT EXISTS(SELECT 1 FROM observed o WHERE o.path=working.path);
        INSERT INTO working SELECT * FROM observed o WHERE NOT EXISTS(SELECT 1 FROM working w WHERE w.path=o.path AND w.blob=o.blob AND w.mode=o.mode AND w.kind=o.kind AND w.size=o.size AND w.modified=o.modified AND w.changed=o.changed)
        ON CONFLICT(path) DO UPDATE SET blob=excluded.blob,mode=excluded.mode,kind=excluded.kind,size=excluded.size,modified=excluded.modified,changed=excluded.changed;")?;
    cancel.check()?;
    transaction.commit()?;
    let mut state = repository.state()?;
    state.created = created;
    state.changed_files = count;
    Ok(state)
}
