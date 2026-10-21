// Purpose: 자동 GC 상태의 단일 실행 잠금·검증·원자적 파일 저장.
// Pattern: Filesystem Adapter.
// Usage: application maintenance가 상태 lease를 소유하고 진행을 기록.
// Related: crates/kodstar-core/src/application/maintenance.rs, crates/kodstar-core/src/infrastructure/cache/locks.rs.

use super::{CacheStore, locks::FileLock};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    results::MaintenanceReport,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedState {
    schema_version: u32,
    pub policy_hash: String,
    pub report: MaintenanceReport,
}
impl SavedState {
    pub(crate) fn new(policy_hash: String, report: MaintenanceReport) -> Self {
        Self {
            schema_version: 1,
            policy_hash,
            report,
        }
    }
}

pub(crate) fn read(home: &Path) -> Result<Option<SavedState>> {
    let path = home.join("gc-state.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::io("inspect GC state", &path, error)),
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(Error::operation("GC state must be a regular file"));
        }
        _ => {}
    }
    let file = File::open(&path).map_err(|error| Error::io("open GC state", &path, error))?;
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::io("read GC state", &path, error))?;
    if bytes.len() > 65536 {
        return Err(Error::operation(
            "GC state exceeds 64 KiB; inspect it before retrying",
        ));
    }
    let state: SavedState = serde_json::from_slice(&bytes).map_err(|error| {
        Error::private_input(
            crate::domain::diagnostics::ErrorKind::Operation,
            "invalid GC state; unknown data was preserved (input excerpt omitted)",
            error,
        )
    })?;
    if state.schema_version != 1 {
        return Err(Error::operation(
            "unsupported GC state version; inspect it before retrying",
        ));
    }
    Ok(Some(state))
}

pub(crate) struct StateLease {
    home: PathBuf,
    _lock: FileLock,
}
impl StateLease {
    pub(crate) fn acquire(store: &CacheStore, cancel: &Cancellation) -> Result<Option<Self>> {
        cancel.check()?;
        match fs::symlink_metadata(&store.home) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(Error::io("inspect automatic GC home", &store.home, error)),
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(Error::operation("cache home must be a real directory"));
            }
            _ => {}
        }
        Ok(
            FileLock::try_acquire(&store.home.join("gc.lock"), false)?.map(|lock| Self {
                home: store.home.clone(),
                _lock: lock,
            }),
        )
    }
    pub(crate) fn write(&self, state: &SavedState) -> Result<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| Error::caused_by("read GC state clock", error))?
            .as_nanos();
        let temporary = self
            .home
            .join(format!(".gc-state-{}-{nonce}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| Error::io("create GC state staging file", &temporary, error))?;
        let result = (|| {
            serde_json::to_writer(&mut file, state)
                .map_err(|error| Error::caused_by("encode GC state", error))?;
            file.flush()
                .and_then(|()| file.sync_all())
                .map_err(|error| Error::io("persist GC state", &temporary, error))?;
            Ok(())
        })();
        drop(file);
        let result = result.and_then(|()| {
            fs::rename(&temporary, self.home.join("gc-state.json")).map_err(|error| {
                Error::io(
                    "publish GC state; inspect state before retrying",
                    &temporary,
                    error,
                )
            })
        });
        if result.is_err() {
            match fs::remove_file(&temporary) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(Error::caused_by(
                        format!(
                            "GC state write failed; unable to remove owned staging file {}",
                            temporary.display()
                        ),
                        error,
                    ));
                }
            }
        }
        result
    }
}
