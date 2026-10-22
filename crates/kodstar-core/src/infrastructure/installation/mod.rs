// Purpose: 실행과 제거의 lease 및 캐시와 독립된 프로젝트 등록부.
// Pattern: Resource Adapter.
// Usage: 공통 실행기와 제거 use case가 호출.
// Related: crates/kodstar-core/src/application/installation.rs, crates/kodstar-core/src/infrastructure/installation/removal.rs.

pub(crate) mod removal;

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::cache::{CacheStore, locks::FileLock, paths};
use rusqlite::{Connection, OpenFlags, params};
use std::{fs, path::Path};

pub(crate) fn lease(
    store: &CacheStore,
    create: bool,
    cancel: &Cancellation,
) -> Result<Option<FileLock>> {
    if !create
        && !store
            .home
            .try_exists()
            .map_err(|e| Error::io("inspect installation", &store.home, e))?
    {
        return Ok(None);
    }
    store.ensure_home()?;
    Ok(Some(FileLock::acquire(
        &store.home.join("installation.lock"),
        true,
        cancel,
    )?))
}

pub(crate) fn registry(store: &CacheStore, create: bool) -> Result<Connection> {
    let path = store.home.join("projects.sqlite");
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err(Error::operation("project registry must be a regular file"));
    }
    let journal = store.home.join("projects.sqlite-journal");
    if fs::symlink_metadata(&journal).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err(Error::operation(
            "project registry journal must be a regular file",
        ));
    }
    let flags = if create {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(&path, flags)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 65536)?;
    connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA cache_size=-256;")?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if create && version == 0 {
        let transaction = rusqlite::Transaction::new_unchecked(
            &connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let version: u32 = transaction.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 0 {
            let occupied: bool =
                transaction.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema)", [], |r| {
                    r.get(0)
                })?;
            if occupied {
                return Err(Error::operation(
                    "unrecognized project registry was preserved",
                ));
            }
            transaction.execute_batch("CREATE TABLE projects(root BLOB PRIMARY KEY, identity TEXT NOT NULL); PRAGMA application_id=1263752274; PRAGMA user_version=1;")?;
        }
        transaction.commit()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|e| Error::io("protect project registry", &path, e))?;
        }
    }
    let application: u32 = connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version != 1 || application != 1263752274 {
        return Err(Error::operation(
            "unsupported project registry; data was preserved",
        ));
    }
    Ok(connection)
}

pub(crate) fn register(store: &CacheStore, root: &Path, identity: &str) -> Result<()> {
    let connection = registry(store, true)?;
    let raw = paths::path_bytes(root);
    let same: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE root=?1 AND identity=?2)",
        params![raw, identity],
        |r| r.get(0),
    )?;
    if !same {
        let transaction = rusqlite::Transaction::new_unchecked(
            &connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        // Copies can share an identity. Retain every existing copy and retire only moved paths.
        let mut statement = transaction.prepare("SELECT root FROM projects WHERE identity=?1")?;
        let mut rows = statement.query([identity])?;
        while let Some(row) = rows.next()? {
            let previous: Vec<u8> = row.get(0)?;
            let path = paths::path_from_bytes(&previous)?;
            if !path
                .try_exists()
                .map_err(|e| Error::io("inspect previous project location", &path, e))?
            {
                transaction.execute("DELETE FROM projects WHERE root=?1", [&previous])?;
            }
        }
        drop(rows);
        drop(statement);
        transaction.execute("INSERT INTO projects(root,identity) VALUES(?1,?2) ON CONFLICT(root) DO UPDATE SET identity=excluded.identity", params![raw, identity])?;
        transaction.commit()?;
    }
    Ok(())
}
