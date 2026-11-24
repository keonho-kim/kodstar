// Purpose: 영구 이력 DB·저장 버전·잠금의 소유권.
// Pattern: Resource Adapter.
// Usage: application/history와 공통 실행 문맥이 저장소를 연다.
// Related: crates/kodstar-core/src/infrastructure/history/schema.sql, crates/kodstar-core/src/infrastructure/cache/locks.rs.

pub(crate) mod changes;
pub(crate) mod checkout;
pub(crate) mod checkpoints;
pub(crate) mod content;
pub(crate) mod inputs;
mod maintenance;
pub(crate) mod paths;
#[cfg(test)]
mod storage_tests;

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    history::HistoryState,
};
use crate::infrastructure::cache::locks::FileLock;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

const VERSION: u32 = 6;
const APPLICATION: u32 = 0x4b534852;
pub(crate) fn recognized_format(application: u32, version: u32) -> bool {
    application == APPLICATION && (matches!(version, 1..=5) || version == VERSION)
}
pub(crate) const CACHE_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const SNAPSHOT_CACHE_BYTES: usize = 2 * 1024 * 1024;

fn configure_cache(connection: &Connection, bytes: usize) -> Result<()> {
    let kib = (bytes / 1024) as i64;
    connection.pragma_update(Some("main"), "cache_size", -(kib * 3 / 4))?;
    connection.pragma_update(Some("temp"), "cache_size", -(kib / 4))?;
    Ok(())
}

pub(crate) struct Repository {
    pub connection: Connection,
    pub root: PathBuf,
    lock: Option<FileLock>,
}

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub database: PathBuf,
    pub id: i64,
}

impl Repository {
    pub fn exists(root: &Path) -> bool {
        root.join(".kodstar/repository.sqlite").is_file()
    }

    pub fn open(root: &Path, create: bool, cancel: &Cancellation) -> Result<Self> {
        Self::open_mode(root, create, false, cancel)
    }

    pub fn upgrade(root: &Path, cancel: &Cancellation) -> Result<Self> {
        Self::open_mode(root, false, true, cancel)
    }

    fn open_mode(root: &Path, create: bool, upgrade: bool, cancel: &Cancellation) -> Result<Self> {
        let directory = root.join(".kodstar");
        if !directory.exists() {
            if !create {
                return Err(Error::not_found(
                    "project is not initialized; run kodstar init first",
                ));
            }
            std::fs::create_dir(&directory)
                .map_err(|e| Error::io("create history directory", &directory, e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                    .map_err(|e| Error::io("protect history directory", &directory, e))?;
            }
        }
        paths::real_directory(&directory)?;
        let database = directory.join("repository.sqlite");
        let fresh = !database.exists();
        if fresh {
            if !create {
                return Err(Error::not_found(
                    "project is not initialized; run kodstar init first",
                ));
            }
            for entry in std::fs::read_dir(&directory)
                .map_err(|e| Error::io("inspect history directory", &directory, e))?
            {
                let entry = entry.map_err(|e| Error::caused_by("inspect history entry", e))?;
                if entry.file_name() != "repository.lock" {
                    return Err(Error::operation(
                        "history directory contains unknown data; refusing initialization",
                    ));
                }
            }
        }
        let lock = FileLock::acquire(&directory.join("repository.lock"), false, cancel)?;
        paths::regular_database(&database)?;
        #[cfg(unix)]
        if fresh {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| Error::io("protect history directory", &directory, e))?;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | if create {
                OpenFlags::SQLITE_OPEN_CREATE
            } else {
                OpenFlags::empty()
            };
        let connection = Connection::open_with_flags(&database, flags)?;
        #[cfg(unix)]
        if fresh {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| Error::io("protect source history", &database, e))?;
        }
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 256 * 1024)?;
        connection.set_prepared_statement_cache_capacity(4);
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA temp_store=FILE;",
        )?;
        configure_cache(&connection, CACHE_BYTES)?;
        content::chunks::register(&connection)?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let application: u32 =
            connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
        if fresh && version == 0 {
            connection.execute_batch("BEGIN IMMEDIATE")?;
            connection.execute_batch(include_str!("schema.sql"))?;
            connection.execute_batch(include_str!("metadata.sql"))?;
            connection.execute_batch(content::chunks::SCHEMA)?;
            let identity = blake3::hash(
                format!(
                    "{}:{:?}:{}",
                    root.display(),
                    std::time::SystemTime::now(),
                    std::process::id()
                )
                .as_bytes(),
            )
            .to_hex()
            .to_string();
            connection.execute("UPDATE repository SET identity=?1", [&identity])?;
            connection.pragma_update(None, "user_version", VERSION)?;
            connection.pragma_update(None, "application_id", APPLICATION)?;
            connection.execute_batch("COMMIT")?;
        } else if upgrade && version != VERSION && recognized_format(application, version) {
            maintenance::run(&connection, cancel)?;
        } else if version != VERSION || application != APPLICATION {
            return Err(Error::operation(
                "unsupported history format; source history was preserved; formats 1 through 5 require kodstar history upgrade",
            ));
        }
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        if fresh {
            for path in [root, directory.as_path()] {
                std::fs::File::open(path)
                    .and_then(|file| file.sync_all())
                    .map_err(|e| Error::io("persist history directory", path, e))?;
            }
        }
        // Finish the bounded initial schema transaction before cancellation can interrupt SQL.
        crate::infrastructure::index::observe_cancellation(&connection, cancel)?;
        let repository = Self {
            connection,
            root: root.to_path_buf(),
            lock: Some(lock),
        };
        if upgrade {
            repository.verify(cancel)?;
            maintenance::repack(&repository.connection, cancel)?;
            maintenance::compact(&repository.connection, cancel)?;
        } else {
            checkout::recover(&repository, cancel)?;
        }
        Ok(repository)
    }

    pub fn snapshot(&self, id: i64) -> Snapshot {
        Snapshot {
            database: self.root.join(".kodstar/repository.sqlite"),
            id,
        }
    }

    pub fn acquire(&mut self, cancel: &Cancellation) -> Result<()> {
        if self.lock.is_none() {
            self.lock = Some(FileLock::acquire(
                &self.root.join(".kodstar/repository.lock"),
                false,
                cancel,
            )?);
            checkout::recover(self, cancel)?;
        }
        Ok(())
    }

    pub fn release(&mut self) {
        self.lock = None;
    }

    pub(crate) fn query_snapshot(&mut self) -> Result<rusqlite::Transaction<'_>> {
        let transaction = self.connection.transaction()?;
        transaction.query_row("SELECT singleton FROM repository", [], |row| {
            row.get::<_, i64>(0)
        })?;
        self.lock = None;
        Ok(transaction)
    }

    pub fn state(&self) -> Result<HistoryState> {
        let (repository, branch, id): (String, String, Option<i64>) = self.connection.query_row(
            "SELECT r.identity,r.current_branch,b.checkpoint FROM repository r JOIN branches b ON b.name=r.current_branch", [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        let tracked_files = self
            .connection
            .query_row("SELECT count(*) FROM working", [], |r| r.get(0))?;
        Ok(HistoryState {
            comparison: None,
            repository,
            branch,
            checkpoint: id.map(checkpoints::display).unwrap_or_default(),
            created: false,
            changed_files: 0,
            tracked_files,
            analysis_checkpoint: None,
            analysis_error: None,
        })
    }

    pub fn identity(&self) -> Result<String> {
        Ok(self
            .connection
            .query_row("SELECT identity FROM repository", [], |r| r.get(0))?)
    }
}

impl Snapshot {
    pub fn open(&self, cancel: &Cancellation) -> Result<Connection> {
        let connection =
            Connection::open_with_flags(&self.database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 256 * 1024)?;
        connection.execute_batch("PRAGMA temp_store=FILE;")?;
        configure_cache(&connection, SNAPSHOT_CACHE_BYTES)?;
        content::chunks::register(&connection)?;
        crate::infrastructure::index::observe_cancellation(&connection, cancel)?;
        connection.execute_batch("BEGIN;")?;
        if checkpoints::head(&connection)? == Some(self.id) {
            connection.execute_batch(
                "CREATE TEMP VIEW snapshot_files AS SELECT path,blob,mode,kind FROM working;",
            )?;
        } else {
            checkpoints::materialize(&connection, self.id)?;
        }
        Ok(connection)
    }
}
