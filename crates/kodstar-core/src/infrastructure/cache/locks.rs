// Purpose: 파일 잠금과 취소 가능한 획득 대기.
// Pattern: RAII Lock.
// Usage: cache가 registry·project 잠금 소유.
// Related: crates/kodstar-core/src/infrastructure/cache/mod.rs.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};

#[derive(Debug)]
pub(crate) struct FileLock {
    _file: File,
}

impl FileLock {
    pub(crate) fn try_acquire(path: &Path, shared: bool) -> Result<Option<Self>> {
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(Error::operation(format!(
                "refusing symlink lock file: {}",
                path.display()
            )));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| Error::io("open cache lock", path, error))?;
        match if shared {
            file.try_lock_shared()
        } else {
            file.try_lock()
        } {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(Error::io("lock cache", path, error)),
        }
    }

    pub(crate) fn acquire(path: &Path, shared: bool, cancel: &Cancellation) -> Result<Self> {
        loop {
            cancel.check()?;
            if let Some(lock) = Self::try_acquire(path, shared)? {
                return Ok(lock);
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}
