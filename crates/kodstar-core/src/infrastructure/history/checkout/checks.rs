// Purpose: 복원 계획·완료 파일의 내용과 경로 충돌 검사.
// Pattern: Filesystem Boundary.
// Usage: checkout이 쓰기 전과 중단 복구 시 상태를 확인.
// Related: crates/kodstar-core/src/infrastructure/history/checkout/files.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::cache::paths::{path_bytes, path_from_bytes};
use crate::infrastructure::history::{Repository, content::blobs, paths};
use rusqlite::OptionalExtension;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

pub(crate) fn preflight(repository: &Repository, cancel: &Cancellation) -> Result<()> {
    let staging = repository.root.join(".kodstar/checkout");
    if staging.exists() {
        paths::real_directory(&staging)?;
        if fs::read_dir(&staging)
            .map_err(|e| Error::io("inspect checkout staging", &staging, e))?
            .next()
            .is_some()
        {
            return Err(Error::operation("checkout staging contains unknown data"));
        }
    }
    let mut statement=repository.connection.prepare("SELECT path,before_blob,before_mode,before_kind,after_blob,after_mode,after_kind FROM checkout_files ORDER BY path")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let path = path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        paths::relative(&path)?;
        #[cfg(unix)]
        same_filesystem(repository, &path)?;
        let before: Option<i64> = row.get(1)?;
        let directory_removal = before.is_none()
            && fs::symlink_metadata(repository.root.join(&path))
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            && super::tree::removable(repository, &path, cancel)?;
        if before.is_none()
            && fs::symlink_metadata(repository.root.join(&path)).is_ok()
            && !directory_removal
        {
            return Err(Error::operation(format!(
                "untracked checkout conflict: {}",
                path.display()
            )));
        }
        let mut parent_removal = false;
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            if let Ok(metadata) = fs::symlink_metadata(repository.root.join(parent))
                && (!metadata.is_dir() || metadata.file_type().is_symlink())
            {
                let removed:bool=repository.connection.query_row("SELECT EXISTS(SELECT 1 FROM checkout_files WHERE path=?1 AND before_blob IS NOT NULL AND after_blob IS NULL)",[path_bytes(parent)],|r|r.get(0))?;
                if !removed {
                    return Err(Error::operation(
                        "checkout parent conflicts with an existing file or symlink",
                    ));
                }
                parent_removal = true;
            }
        }
        if !directory_removal
            && !(before.is_none() && parent_removal)
            && !matches(
                repository,
                &path,
                before,
                row.get(2)?,
                row.get::<_, Option<String>>(3)?.as_deref(),
                cancel,
            )?
        {
            return Err(Error::operation(format!(
                "working file changed before checkout: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn same_filesystem(repository: &Repository, path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let staging = repository.root.join(".kodstar");
    let expected = fs::metadata(&staging)
        .map_err(|e| Error::io("inspect checkout staging device", &staging, e))?
        .dev();
    let mut current = repository.root.clone();
    let mut device = fs::metadata(&current)
        .map_err(|e| Error::io("inspect project device", &current, e))?
        .dev();
    for component in path.parent().unwrap_or_else(|| Path::new("")).components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                device = metadata.dev();
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                break;
            }
            Err(e) => return Err(Error::io("inspect checkout device", &current, e)),
        }
    }
    if device != expected {
        return Err(Error::operation(
            "checkout across filesystems is unsupported; working files were preserved",
        ));
    }
    Ok(())
}

pub(super) fn verify_completed(
    repository: &Repository,
    only_completed: bool,
    cancel: &Cancellation,
) -> Result<()> {
    let mut statement=repository.connection.prepare("SELECT path,after_blob,after_mode,after_kind FROM checkout_files WHERE ?1=0 OR done=1 ORDER BY path")?;
    let mut rows = statement.query([only_completed])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let path = path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        if !matches(
            repository,
            &path,
            row.get(1)?,
            row.get(2)?,
            row.get::<_, Option<String>>(3)?.as_deref(),
            cancel,
        )? {
            return Err(Error::operation(format!(
                "completed checkout file changed; recovery stopped: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

pub(super) fn matches(
    repository: &Repository,
    path: &Path,
    blob: Option<i64>,
    mode: Option<u32>,
    kind: Option<&str>,
    cancel: &Cancellation,
) -> Result<bool> {
    paths::relative(path)?;
    let absolute = repository.root.join(path);
    let metadata = match fs::symlink_metadata(&absolute) {
        Ok(m) => m,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(blob.is_none());
        }
        Err(e) => return Err(Error::io("inspect checkout file", &absolute, e)),
    };
    let Some(blob) = blob else {
        return Ok(metadata.is_dir() && !metadata.file_type().is_symlink());
    };
    let expected: Option<String> = repository
        .connection
        .query_row("SELECT hash FROM blobs WHERE id=?1", [blob], |r| r.get(0))
        .optional()?;
    let Some(expected) = expected else {
        return Err(Error::operation("checkout references missing content"));
    };
    let mut hash = blake3::Hasher::new();
    if metadata.file_type().is_symlink() && kind == Some("symlink") {
        hash.update(&path_bytes(
            &fs::read_link(&absolute)
                .map_err(|e| Error::io("inspect checkout symlink", &absolute, e))?,
        ));
    } else if metadata.is_file() && kind == Some("file") {
        paths::destination(&repository.root, path)?;
        if mode != Some(crate::infrastructure::history::paths::file_mode(&metadata)) {
            return Ok(false);
        }
        let mut file =
            File::open(&absolute).map_err(|e| Error::io("verify checkout file", &absolute, e))?;
        let mut buffer = [0u8; blobs::CHUNK];
        loop {
            cancel.check()?;
            let n = file
                .read(&mut buffer)
                .map_err(|e| Error::io("verify checkout bytes", &absolute, e))?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
    } else {
        return Ok(false);
    }
    Ok(hash.finalize().to_hex().as_str() == expected)
}
