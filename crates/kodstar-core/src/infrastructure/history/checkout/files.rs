// Purpose: 확인된 복원 계획의 파일 반영과 내구성 확보.
// Pattern: Filesystem Adapter.
// Usage: checkout이 파일별 journal을 재개하여 안전하게 반영.
// Related: crates/kodstar-core/src/infrastructure/history/checkout/checks.rs.

use super::checks::{matches, verify_completed};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::cache::paths::path_from_bytes;
use crate::infrastructure::history::{Repository, content::blobs, paths};
use std::fs::{self, File, OpenOptions};

pub(crate) fn apply(repository: &Repository, cancel: &Cancellation) -> Result<()> {
    verify_completed(repository, true, cancel)?;
    let connection = &repository.connection;
    let mut statement=connection.prepare("SELECT path,before_blob,before_mode,before_kind,after_blob,after_mode,after_kind FROM checkout_files WHERE done=0 ORDER BY after_blob IS NOT NULL,length(path) DESC,path")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let raw: Vec<u8> = row.get(0)?;
        let path = path_from_bytes(&raw)?;
        let before: Option<i64> = row.get(1)?;
        let after: Option<i64> = row.get(4)?;
        let after_mode: Option<u32> = row.get(5)?;
        let after_kind: Option<String> = row.get(6)?;
        let directory_removal = before.is_none()
            && after.is_some()
            && fs::symlink_metadata(repository.root.join(&path))
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            && super::tree::removable(repository, &path, cancel)?;
        if !matches(
            repository,
            &path,
            after,
            after_mode,
            after_kind.as_deref(),
            cancel,
        )? {
            if !directory_removal
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
                    "checkout recovery conflict; file preserved: {}",
                    path.display()
                )));
            }
            let destination = paths::destination(&repository.root, &path)?;
            if directory_removal {
                super::tree::remove_empty(&destination, 0, cancel)?;
            }
            if let Some(blob) = after {
                let parent = destination
                    .parent()
                    .ok_or_else(|| Error::operation("checkout path lacks parent"))?;
                fs::create_dir_all(parent)
                    .map_err(|e| Error::io("create checkout parent", parent, e))?;
                paths::destination(&repository.root, &path)?;
                let staging = repository.root.join(".kodstar/checkout");
                if !staging.exists() {
                    fs::create_dir(&staging)
                        .map_err(|e| Error::io("create checkout staging", &staging, e))?;
                }
                paths::real_directory(&staging)?;
                let temporary = staging.join(blake3::hash(&raw).to_hex().as_str());
                if let Ok(metadata) = fs::symlink_metadata(&temporary) {
                    if !metadata.is_file() && !metadata.file_type().is_symlink() {
                        return Err(Error::operation("unsafe checkout staging file"));
                    }
                    fs::remove_file(&temporary)
                        .map_err(|e| Error::io("discard incomplete staging file", &temporary, e))?;
                }
                if after_kind.as_deref() == Some("symlink") {
                    let target =
                        path_from_bytes(&blobs::read(connection, blob, 1024 * 1024, cancel)?)?;
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&target, &temporary)
                        .map_err(|e| Error::io("stage symlink", &temporary, e))?;
                } else {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&temporary)
                        .map_err(|e| Error::io("stage history file", &temporary, e))?;
                    blobs::write(connection, blob, &mut file, cancel)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        file.set_permissions(fs::Permissions::from_mode(
                            after_mode.unwrap_or(0o644),
                        ))
                        .map_err(|e| Error::io("set restored permissions", &temporary, e))?;
                    }
                    file.sync_all()
                        .map_err(|e| Error::io("persist restored file", &temporary, e))?;
                }
                fs::rename(&temporary, &destination)
                    .map_err(|e| Error::io("publish restored file", &destination, e))?;
            } else if fs::symlink_metadata(&destination).is_ok() {
                fs::remove_file(&destination)
                    .map_err(|e| Error::io("remove tracked file", &destination, e))?;
            }
        }
        let parent = match paths::destination(&repository.root, &path) {
            Ok(destination) => destination
                .parent()
                .map(|p| p.to_path_buf())
                .ok_or_else(|| Error::operation("checkout destination lacks a parent"))?,
            // A deleted child can now sit below a directory replaced by a tracked file.
            Err(_) if after.is_none() => repository.root.clone(),
            Err(error) => return Err(error),
        };
        File::open(&parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| Error::io("persist checkout directory", &parent, e))?;
        connection.execute("UPDATE checkout_files SET done=1 WHERE path=?1", [raw])?;
    }
    verify_completed(repository, false, cancel)
}
