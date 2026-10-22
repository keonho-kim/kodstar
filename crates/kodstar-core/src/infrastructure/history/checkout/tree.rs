// Purpose: 파일·디렉터리 전환에서 추적하지 않은 내용을 보존.
// Pattern: Filesystem Adapter.
// Usage: checkout_files가 제거 가능한 디렉터리만 확인·정리.
// Related: crates/kodstar-core/src/infrastructure/history/checkout/files.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, Result},
};
use crate::infrastructure::cache::paths::path_bytes;
use crate::infrastructure::history::Repository;
use std::fs;
use std::path::Path;

pub(super) fn removable(
    repository: &Repository,
    path: &Path,
    cancel: &Cancellation,
) -> Result<bool> {
    check(repository, path, 0, cancel)
}

fn check(
    repository: &Repository,
    path: &Path,
    depth: usize,
    cancel: &Cancellation,
) -> Result<bool> {
    cancel.check()?;
    if depth > 128 {
        return Err(Error::new(
            ErrorKind::Limit,
            "checkout directory depth exceeds 128",
        ));
    }
    let absolute = repository.root.join(path);
    let metadata = fs::symlink_metadata(&absolute)
        .map_err(|e| Error::io("inspect checkout directory", &absolute, e))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(repository.connection.query_row("SELECT EXISTS(SELECT 1 FROM checkout_files WHERE path=?1 AND before_blob IS NOT NULL AND after_blob IS NULL)",[path_bytes(path)],|r|r.get(0))?);
    }
    for entry in fs::read_dir(&absolute)
        .map_err(|e| Error::io("inspect checkout directory contents", &absolute, e))?
    {
        let entry = entry.map_err(|e| Error::caused_by("inspect checkout directory entry", e))?;
        if !check(repository, &path.join(entry.file_name()), depth + 1, cancel)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn remove_empty(path: &Path, depth: usize, cancel: &Cancellation) -> Result<()> {
    cancel.check()?;
    if depth > 128 {
        return Err(Error::new(
            ErrorKind::Limit,
            "checkout directory depth exceeds 128",
        ));
    }
    for entry in
        fs::read_dir(path).map_err(|e| Error::io("read empty checkout directory", path, e))?
    {
        let entry = entry.map_err(|e| Error::caused_by("read checkout directory entry", e))?;
        let metadata = entry
            .file_type()
            .map_err(|e| Error::caused_by("inspect checkout entry type", e))?;
        if !metadata.is_dir() || metadata.is_symlink() {
            return Err(Error::operation(
                "checkout directory contains untracked content",
            ));
        }
        remove_empty(&entry.path(), depth + 1, cancel)?;
    }
    fs::remove_dir(path).map_err(|e| Error::io("remove empty checkout directory", path, e))
}
