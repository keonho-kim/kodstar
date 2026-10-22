// Purpose: 복원·브랜치 전환의 지속적인 작업 계획과 중단 복구.
// Pattern: Recoverable State Transition.
// Usage: application/history가 현재 상태 보존 후 호출하고 저장소 재개 시 복구.
// Related: crates/kodstar-core/src/infrastructure/history/checkout/files.rs.

mod checks;
mod files;
mod tree;

use super::{Repository, checkpoints};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::cache::{
    now_seconds,
    paths::{path_bytes, path_from_bytes},
};
use rusqlite::{OptionalExtension, params};
use std::path::Path;

pub(crate) fn start(
    repository: &Repository,
    target: i64,
    branch: Option<&str>,
    path: Option<&Path>,
    cancel: &Cancellation,
) -> Result<u64> {
    if let Some(path) = path {
        super::paths::relative(path)?;
    }
    checkpoints::materialize(&repository.connection, target)?;
    let connection = &repository.connection;
    connection.execute_batch("BEGIN IMMEDIATE; DELETE FROM checkout_files;")?;
    let result: Result<()> = (|| {
        connection.execute(
            "INSERT INTO checkout VALUES(1,?1,?2,?3)",
            params![target, branch, path.map(path_bytes)],
        )?;
        let mut statement=connection.prepare("SELECT w.path,w.blob,w.mode,w.kind,s.blob,s.mode,s.kind FROM working w LEFT JOIN snapshot_files s ON s.path=w.path UNION ALL SELECT s.path,NULL,NULL,NULL,s.blob,s.mode,s.kind FROM snapshot_files s WHERE NOT EXISTS(SELECT 1 FROM working w WHERE w.path=s.path)")?;
        let mut rows = statement.query([])?;
        let mut found = path.is_none();
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let raw: Vec<u8> = row.get(0)?;
            let file = path_from_bytes(&raw)?;
            super::paths::relative(&file)?;
            if path.is_some_and(|selected| !file.starts_with(selected)) {
                continue;
            }
            found = true;
            let before: Option<i64> = row.get(1)?;
            let after: Option<i64> = row.get(4)?;
            let before_mode: Option<u32> = row.get(2)?;
            let after_mode: Option<u32> = row.get(5)?;
            let before_kind: Option<String> = row.get(3)?;
            let after_kind: Option<String> = row.get(6)?;
            if before == after && before_mode == after_mode && before_kind == after_kind {
                continue;
            }
            connection.execute(
                "INSERT INTO checkout_files VALUES(?1,?2,?3,?4,?5,?6,?7,0)",
                params![
                    raw,
                    before,
                    before_mode,
                    before_kind,
                    after,
                    after_mode,
                    after_kind
                ],
            )?;
        }
        if !found {
            return Err(Error::not_found(
                "restore path is absent from both tracked states",
            ));
        }
        checks::preflight(repository, cancel)?;
        Ok(())
    })();
    match result {
        Ok(()) => connection.execute_batch("COMMIT")?,
        Err(error) => {
            connection.execute_batch("ROLLBACK")?;
            return Err(error);
        }
    }
    let count = connection.query_row("SELECT count(*) FROM checkout_files", [], |r| r.get(0))?;
    recover(repository, cancel)?;
    Ok(count)
}

pub(crate) fn recover(repository: &Repository, cancel: &Cancellation) -> Result<()> {
    let pending: Option<(i64, Option<String>)> = repository
        .connection
        .query_row("SELECT target,branch FROM checkout", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let Some((target, branch)) = pending else {
        return Ok(());
    };
    files::apply(repository, cancel)?;
    cancel.check()?;
    let connection = &repository.connection;
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result: Result<()> = (|| {
        if let Some(branch) = branch {
            let actual: Option<i64> = connection.query_row(
                "SELECT checkpoint FROM branches WHERE name=?1",
                [&branch],
                |r| r.get(0),
            )?;
            if actual != Some(target) {
                return Err(Error::operation("branch changed during checkout recovery"));
            }
            checkpoints::materialize(connection, target)?;
            connection.execute_batch("DELETE FROM working; INSERT INTO working SELECT f.path,f.blob,f.mode,f.kind,b.size,'','' FROM snapshot_files f JOIN blobs b ON b.id=f.blob;")?;
            connection.execute("UPDATE repository SET current_branch=?1", [branch])?;
        } else {
            let count: u64 =
                connection.query_row("SELECT count(*) FROM checkout_files", [], |r| r.get(0))?;
            if count > 0 {
                connection.execute(
                    "INSERT INTO checkpoints(parent,created_at,changed_files,tracked_files) VALUES(?1,?2,?3,(SELECT count(*) FROM working)+(SELECT count(*) FROM checkout_files WHERE before_blob IS NULL AND after_blob IS NOT NULL)-(SELECT count(*) FROM checkout_files WHERE before_blob IS NOT NULL AND after_blob IS NULL))",
                    params![checkpoints::head(connection)?, now_seconds()?, count],
                )?;
                let id = connection.last_insert_rowid();
                connection.execute("INSERT INTO entries SELECT ?1,path,after_blob,coalesce(after_mode,before_mode),coalesce(after_kind,before_kind) FROM checkout_files",[id])?;
                connection.execute("UPDATE branches SET checkpoint=?1 WHERE name=(SELECT current_branch FROM repository)",[id])?;
                connection.execute_batch("DELETE FROM working WHERE path IN (SELECT path FROM checkout_files); INSERT INTO working SELECT f.path,f.after_blob,f.after_mode,f.after_kind,b.size,'','' FROM checkout_files f JOIN blobs b ON b.id=f.after_blob;")?;
            }
        }
        connection.execute_batch("DELETE FROM checkout_files; DELETE FROM checkout;")?;
        Ok(())
    })();
    match result {
        Ok(()) => connection.execute_batch("COMMIT")?,
        Err(error) => {
            connection.execute_batch("ROLLBACK")?;
            return Err(error);
        }
    }
    Ok(())
}
