// Purpose: cache 목록·용량·활성 상태를 임시 SQLite에 수집하고 순차 조회.
// Pattern: Storage Adapter.
// Usage: application cache 정책이 전체 목록을 메모리에 모으지 않고 소비.
// Related: crates/kodstar-core/src/application/cache.rs, crates/kodstar-core/src/infrastructure/cache/inspection.rs.

use super::{
    CacheStore,
    inspection::{ScanBudget, inspect},
    locks::FileLock,
    paths::{encoding, path_bytes, path_from_bytes},
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    results::CacheEntry,
};
use rusqlite::{Connection, params};
use std::fs;

pub(crate) struct Record {
    pub key: String,
    pub root: Option<std::path::PathBuf>,
    pub safe: bool,
    pub entry: CacheEntry,
}

pub(crate) struct Catalog {
    pub complete: bool,
    pub stop_reason: Option<&'static str>,
    connection: Connection,
}

impl Catalog {
    pub(crate) fn scan(
        store: &CacheStore,
        cancel: &Cancellation,
        mut budget: Option<&mut ScanBudget>,
    ) -> Result<Self> {
        let connection = Connection::open("")?;
        connection.execute_batch("PRAGMA temp_store=FILE; PRAGMA cache_size=-1024; CREATE TABLE caches(cache_key TEXT PRIMARY KEY,root BLOB,bytes INTEGER,last_used INTEGER,active INTEGER,safe INTEGER,reason TEXT,bytes_complete INTEGER);")?;
        let mut result = Self {
            connection,
            complete: true,
            stop_reason: None,
        };
        match fs::symlink_metadata(&store.home) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
            Err(error) => return Err(Error::io("inspect cache home", &store.home, error)),
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(Error::operation("cache home must be a real directory"));
            }
            _ => {}
        }
        let registry = if budget.is_some() {
            FileLock::try_acquire(&store.home.join("registry.lock"), true)?
        } else {
            Some(FileLock::acquire(
                &store.home.join("registry.lock"),
                true,
                cancel,
            )?)
        };
        let Some(_registry) = registry else {
            result.complete = false;
            result.stop_reason = Some("maintenance_registry_busy");
            return Ok(result);
        };
        let mut projects = 0;
        result.connection.execute_batch("BEGIN")?;
        for item in fs::read_dir(&store.home)
            .map_err(|error| Error::io("list project caches", &store.home, error))?
        {
            cancel.check()?;
            if budget
                .as_deref_mut()
                .is_some_and(|budget| !budget.advance())
            {
                result.complete = false;
                result.stop_reason = Some("maintenance_scan_limit");
                break;
            }

            let item = item.map_err(|error| Error::io("read project cache", &store.home, error))?;
            let name = item.file_name();
            let Some(key) = name.to_str().filter(|name| {
                name.len() == 64
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            }) else {
                continue;
            };
            let kind = item
                .file_type()
                .map_err(|error| Error::io("inspect project cache", &item.path(), error))?;
            if !kind.is_dir() || kind.is_symlink() {
                continue;
            }
            if budget.is_some() && projects == 64 {
                result.complete = false;
                result.stop_reason = Some("maintenance_scan_limit");
                break;
            }
            projects += 1;
            let directory = item.path();
            let mut state = inspect(&directory, cancel, budget.as_deref_mut())?;
            if state.reason == Some("scan_limit") {
                result.complete = false;
                result.stop_reason = Some("maintenance_scan_limit");
                break;
            }
            let marker = directory.join("project.json");
            let record = if fs::symlink_metadata(&marker)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            {
                store.read_record(&directory).ok()
            } else {
                None
            };
            let (root, last_used, active) = if let Some((root, record)) = record {
                let lease = FileLock::try_acquire(&directory.join("lease.lock"), false)?;
                (Some(path_bytes(&root)), record.last_used, lease.is_none())
            } else {
                state.safe = false;
                state.reason = Some("invalid_registration");
                (None, 0, false)
            };
            result.connection.execute(
                "INSERT INTO caches VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    key,
                    root,
                    state.bytes,
                    last_used,
                    active,
                    state.safe,
                    state.reason,
                    state.bytes_complete
                ],
            )?;
        }
        result.connection.execute_batch("COMMIT")?;
        Ok(result)
    }

    pub(crate) fn size_complete(&self) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM caches WHERE bytes_complete=0)",
            [],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn total_bytes(&self) -> Result<u64> {
        Ok(self
            .connection
            .query_row("SELECT COALESCE(SUM(bytes),0) FROM caches", [], |row| {
                row.get(0)
            })?)
    }

    pub(crate) fn visit(
        &self,
        store: &CacheStore,
        consume: &mut dyn FnMut(Record) -> Result<()>,
    ) -> Result<()> {
        let mut statement = self.connection.prepare("SELECT cache_key,root,bytes,last_used,active,safe,reason,bytes_complete FROM caches ORDER BY last_used,cache_key")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let key: String = row.get(0)?;
            let raw: Option<Vec<u8>> = row.get(1)?;
            let root = raw.map(|bytes| path_from_bytes(&bytes)).transpose()?;
            let directory = store.home.join(&key);
            let reason: Option<String> = row.get(6)?;
            let reason = match reason.as_deref() {
                Some("unknown_data") => Some("unknown_data"),
                Some("invalid_registration") => Some("invalid_registration"),
                _ => None,
            };
            consume(Record {
                key,
                safe: row.get(5)?,
                entry: CacheEntry {
                    root: root
                        .as_ref()
                        .map(|root| root.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    root_encoding: root.as_deref().and_then(encoding),
                    cache: directory.to_string_lossy().into_owned(),
                    cache_encoding: encoding(&directory),
                    bytes: row.get(2)?,
                    last_used: row.get(3)?,
                    active: row.get(4)?,
                    bytes_complete: row.get(7)?,
                    action: "keep",
                    reason,
                },
                root,
            })?;
        }
        Ok(())
    }
}
