// Purpose: Managed cache directory safety, byte inspection and scan budget.
// Pattern: Filesystem Adapter.
// Usage: Cache catalog and removal inspect entries under their existing leases.
// Related: crates/kodstar-core/src/infrastructure/cache/catalog.rs, crates/kodstar-core/src/infrastructure/cache/removal.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use std::{fs, path::Path};

pub(super) const MANAGED_FILES: &[&str] = &[
    "project.json",
    "index.sqlite",
    "index.sqlite-wal",
    "index.sqlite-shm",
    "past.sqlite",
    "past.sqlite-wal",
    "past.sqlite-shm",
    "graph-diff.sqlite",
    "graph-diff.sqlite-wal",
    "graph-diff.sqlite-shm",
    "writer.lock",
    "lease.lock",
];

pub(crate) struct ScanBudget {
    remaining: usize,
    deadline: std::time::Instant,
}
impl ScanBudget {
    pub(crate) fn new() -> Self {
        Self {
            remaining: 512,
            deadline: std::time::Instant::now() + std::time::Duration::from_millis(50),
        }
    }
    pub(crate) fn deadline(&self) -> std::time::Instant {
        self.deadline
    }
    pub(super) fn advance(&mut self) -> bool {
        if self.remaining == 0 || std::time::Instant::now() >= self.deadline {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

pub(crate) struct Inspection {
    pub bytes: u64,
    pub safe: bool,
    pub bytes_complete: bool,
    pub reason: Option<&'static str>,
}

pub(crate) fn inspect(
    directory: &Path,
    cancel: &Cancellation,
    mut budget: Option<&mut ScanBudget>,
) -> Result<Inspection> {
    let mut result = Inspection {
        bytes: 0,
        safe: true,
        bytes_complete: true,
        reason: None,
    };
    for entry in fs::read_dir(directory)
        .map_err(|error| Error::io("inspect cache directory", directory, error))?
    {
        cancel.check()?;
        if budget
            .as_deref_mut()
            .is_some_and(|budget| !budget.advance())
        {
            result.safe = false;
            result.bytes_complete = false;
            result.reason = Some("scan_limit");
            break;
        }

        let entry = entry.map_err(|error| Error::io("read cache entry", directory, error))?;
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                result.bytes_complete = false;
                continue;
            }
            Err(error) => return Err(Error::io("inspect cache entry", &entry.path(), error)),
        };
        let known = entry
            .file_name()
            .to_str()
            .is_some_and(|name| MANAGED_FILES.contains(&name));
        if !known || !metadata.is_file() || metadata.file_type().is_symlink() {
            result.safe = false;
            result.reason = Some("unknown_data");
            result.bytes_complete = false;
        }
        result.bytes = result
            .bytes
            .checked_add(metadata.len())
            .ok_or_else(|| Error::operation("cache byte count overflow"))?;
        if !result.safe {
            return Ok(result);
        }
    }
    Ok(result)
}
