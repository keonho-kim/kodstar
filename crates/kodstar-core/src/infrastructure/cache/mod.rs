// Purpose: 프로젝트 cache 등록·lease·사용 시각 I/O.
// Pattern: Resource Adapter.
// Usage: workspace와 Engine이 작업 수명 동안 사용.
// Related: crates/kodstar-core/src/infrastructure/cache/paths.rs, crates/kodstar-core/src/infrastructure/cache/locks.rs.

//! Cache ownership and filesystem leases. A registry lock closes the open-versus-GC race.

pub(crate) mod catalog;
pub(crate) mod inspection;
pub(crate) mod locks;
pub(crate) mod maintenance;
pub(crate) mod paths;
pub(crate) mod removal;

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};

use locks::FileLock;
use paths::{PLATFORM, cache_key, path_bytes, path_from_bytes};

#[derive(Clone, Debug)]
pub(crate) struct CacheStore {
    pub home: PathBuf,
}

#[derive(Debug)]
pub(crate) struct ProjectCache {
    pub directory: PathBuf,
    pub root: PathBuf,
    _lease: FileLock,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectRecord {
    pub cache_key_version: u32,
    pub platform: String,
    pub root: String,
    pub root_bytes: Vec<u8>,
    pub last_used: u64,
}

pub(crate) fn now_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| Error::caused_by("system clock precedes Unix epoch", error))
}

impl CacheStore {
    pub(crate) fn ensure_home(&self) -> Result<()> {
        let mut builder = DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&self.home)
            .map_err(|error| Error::io("create cache home", &self.home, error))?;
        if fs::symlink_metadata(&self.home)
            .map_err(|error| Error::io("inspect cache home", &self.home, error))?
            .file_type()
            .is_symlink()
        {
            return Err(Error::operation(
                "cache home must be a real directory, not a symlink",
            ));
        }
        Ok(())
    }

    pub(crate) fn registered(&self, root: &Path, cancel: &Cancellation) -> Result<bool> {
        if !self.home.is_dir() {
            return Ok(false);
        }
        let _registry = FileLock::acquire(&self.home.join("registry.lock"), true, cancel)?;
        let directory = self.home.join(cache_key(root));
        if !directory
            .join("project.json")
            .try_exists()
            .map_err(|error| Error::io("inspect project registration", &directory, error))?
        {
            return Ok(false);
        }
        let _lease = FileLock::acquire(&directory.join("lease.lock"), true, cancel)?;
        let (stored, _) = self.read_record(&directory)?;
        if stored != root {
            return Err(Error::operation(
                "cache root does not match its path fingerprint",
            ));
        }
        Ok(directory.join("index.sqlite").is_file())
    }

    pub(crate) fn acquire(
        &self,
        root: &Path,
        create: bool,
        cancel: &Cancellation,
    ) -> Result<ProjectCache> {
        if create {
            self.ensure_home()?;
        }
        if !self.home.is_dir() {
            return Err(Error::not_found("no index found; run kodstar build first"));
        }
        let _registry = FileLock::acquire(&self.home.join("registry.lock"), true, cancel)?;
        let directory = self.home.join(cache_key(root));
        if create {
            let mut builder = DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(Error::io("create project cache", &directory, error)),
            }
        }
        let metadata = fs::symlink_metadata(&directory).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::not_found("no index found; run kodstar build first")
            } else {
                Error::io("inspect project cache", &directory, error)
            }
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::operation("project cache is not a real directory"));
        }
        let lease = FileLock::acquire(&directory.join("lease.lock"), true, cancel)?;
        if directory.join("project.json").is_file() {
            let (stored, _) = self.read_record(&directory)?;
            if stored != root {
                return Err(Error::operation("cache root fingerprint mismatch"));
            }
        } else if !create {
            return Err(Error::not_found("no index found; run kodstar build first"));
        }
        Ok(ProjectCache {
            directory,
            root: root.to_path_buf(),
            _lease: lease,
        })
    }

    pub(crate) fn read_record(&self, directory: &Path) -> Result<(PathBuf, ProjectRecord)> {
        let path = directory.join("project.json");
        let file = File::open(&path)
            .map_err(|error| Error::io("open project registration", &path, error))?;
        let modified = file
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|error| Error::io("read cache usage time", &path, error))?;
        let mut bytes = Vec::new();
        file.take(65537)
            .read_to_end(&mut bytes)
            .map_err(|error| Error::io("read project registration", &path, error))?;
        if bytes.len() > 65536 {
            return Err(Error::operation("project registration exceeds 64 KiB"));
        }
        let mut record: ProjectRecord = serde_json::from_slice(&bytes).map_err(|error| {
            Error::private_input(
                crate::domain::diagnostics::ErrorKind::Operation,
                "invalid project registration (input excerpt omitted)",
                error,
            )
        })?;
        if record.cache_key_version != 1 || record.platform != PLATFORM {
            return Err(Error::operation(
                "unsupported cache key version or path platform; recreate the cache explicitly",
            ));
        }
        let root = path_from_bytes(&record.root_bytes)?;
        if !root.is_absolute()
            || directory.file_name().and_then(|name| name.to_str())
                != Some(cache_key(&root).as_str())
        {
            return Err(Error::operation(
                "project registration has an invalid root fingerprint",
            ));
        }
        if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
            record.last_used = record.last_used.max(duration.as_secs());
        }
        Ok((root, record))
    }
}

impl ProjectCache {
    pub(crate) fn writer(&self, cancel: &Cancellation) -> Result<FileLock> {
        FileLock::acquire(&self.directory.join("writer.lock"), false, cancel)
    }

    pub(crate) fn initialize_record(&self) -> Result<()> {
        let path = self.directory.join("project.json");
        if path.is_file() {
            return Ok(());
        }
        for entry in fs::read_dir(&self.directory)
            .map_err(|error| Error::io("inspect new cache", &self.directory, error))?
        {
            let entry = entry
                .map_err(|error| Error::io("inspect new cache entry", &self.directory, error))?;
            if !matches!(
                entry.file_name().to_str(),
                Some("lease.lock" | "writer.lock")
            ) {
                return Err(Error::operation(
                    "unregistered cache directory contains unknown data; refusing to overwrite it",
                ));
            }
        }
        let record = ProjectRecord {
            cache_key_version: 1,
            platform: PLATFORM.into(),
            root: self.root.to_string_lossy().into_owned(),
            root_bytes: path_bytes(&self.root),
            last_used: now_seconds()?,
        };
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| Error::io("create project registration", &path, error))?;
        serde_json::to_writer(&mut file, &record)
            .map_err(|error| Error::caused_by("write project registration", error))?;
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|error| Error::io("persist project registration", &path, error))
    }

    pub(crate) fn touch(&self) -> Result<()> {
        let path = self.directory.join("project.json");
        File::open(&path)
            .and_then(|file| file.set_modified(SystemTime::now()))
            .map_err(|error| Error::io("record cache use", &path, error))
    }
}
