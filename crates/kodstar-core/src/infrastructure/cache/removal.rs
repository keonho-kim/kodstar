// Purpose: registry·lease 재확인 후 관리되는 cache 파일만 삭제.
// Pattern: Filesystem Adapter.
// Usage: application cache 정책이 선택한 후보의 삭제를 요청.
// Related: crates/kodstar-core/src/infrastructure/cache/inspection.rs, crates/kodstar-core/src/infrastructure/cache/locks.rs.

use super::{
    CacheStore,
    inspection::{self, MANAGED_FILES},
    locks::FileLock,
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use std::fs;
use std::path::Path;

pub(crate) enum Removal {
    Removed,
    Protected(&'static str),
}

pub(crate) fn remove(
    store: &CacheStore,
    key: &str,
    expected_root: &Path,
    expected_last_used: Option<u64>,
    wait: bool,
    cancel: &Cancellation,
) -> Result<Removal> {
    let registry = if wait {
        Some(FileLock::acquire(
            &store.home.join("registry.lock"),
            false,
            cancel,
        )?)
    } else {
        FileLock::try_acquire(&store.home.join("registry.lock"), false)?
    };
    let Some(_registry) = registry else {
        return Ok(Removal::Protected("registry_busy"));
    };
    let directory = store.home.join(key);
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Removal::Protected("already_absent"));
        }
        Err(error) => return Err(Error::io("inspect removal target", &directory, error)),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(Removal::Protected("unknown_data"));
    }
    let Some(lease) = FileLock::try_acquire(&directory.join("lease.lock"), false)? else {
        return Ok(Removal::Protected("active"));
    };
    let inspection = inspection::inspect(&directory, cancel, None)?;
    if !inspection.safe {
        return Ok(Removal::Protected("unknown_data"));
    }
    let (root, record) = store.read_record(&directory)?;
    if expected_last_used.is_some_and(|expected| record.last_used != expected) {
        return Ok(Removal::Protected("recently_used"));
    }
    if root != expected_root {
        return Err(Error::operation("cache root changed before removal"));
    }
    cancel.check()?;
    // The exclusive registry prevents new leases. Close the lease handle before unlinking on Windows.
    drop(lease);
    for name in MANAGED_FILES
        .iter()
        .copied()
        .filter(|name| *name != "project.json")
    {
        remove_file(&directory.join(name))?;
    }
    remove_file(&directory.join("project.json"))?;
    fs::remove_dir(&directory).map_err(|error| {
        Error::io(
            "finish cache removal; inspect remaining files before retrying",
            &directory,
            error,
        )
    })?;
    Ok(Removal::Removed)
}

fn remove_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(
            "remove managed cache file; cleanup may be partial",
            path,
            error,
        )),
    }
}
