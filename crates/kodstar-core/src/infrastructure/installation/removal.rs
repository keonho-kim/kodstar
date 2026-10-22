// Purpose: 등록된 이력·캐시의 소유권 확인과 제한된 삭제.
// Pattern: Filesystem Adapter.
// Usage: application installation이 exclusive 실행 lease를 소유하고 호출.
// Related: crates/kodstar-core/src/infrastructure/installation/mod.rs, crates/kodstar-core/src/infrastructure/cache/inspection.rs.

use super::registry;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::cache::{CacheStore, inspection, locks::FileLock, paths};
use rusqlite::{Connection, OpenFlags};
use std::{
    fs,
    path::{Path, PathBuf},
};

const HOME_FILES: &[&str] = &[
    "config.toml",
    "gc-state.json",
    "gc.lock",
    "registry.lock",
    "installation.lock",
    "install.json",
    "projects.sqlite",
    "projects.sqlite-journal",
];
const HISTORY_FILES: &[&str] = &[
    "repository.sqlite",
    "repository.sqlite-wal",
    "repository.sqlite-shm",
    "repository.sqlite-journal",
    "repository.lock",
];

pub(crate) fn run(store: &CacheStore, cancel: &Cancellation) -> Result<()> {
    let path = store.home.join("projects.sqlite");
    let projects = if path.exists() {
        Some(registry(store, false)?)
    } else {
        None
    };
    // Validate all locations before deleting the first checkpoint history.
    home(store, false, cancel)?;
    if let Some(projects) = &projects {
        visit(projects, &mut |root, id| history(&root, id, false, cancel))?;
    }
    if let Some(projects) = &projects {
        visit(projects, &mut |root, id| history(&root, id, true, cancel))?;
    }
    drop(projects);
    home(store, true, cancel)?;
    // Cache removal uses this lock, so unlink it after the last cache has gone.
    let registry_lock = store.home.join("registry.lock");
    if registry_lock
        .try_exists()
        .map_err(|e| Error::io("inspect final registry lock", &registry_lock, e))?
    {
        regular(&registry_lock)?;
        remove_file(&registry_lock)?;
    }
    Ok(())
}

fn visit(
    connection: &Connection,
    consume: &mut dyn FnMut(PathBuf, &str) -> Result<()>,
) -> Result<()> {
    let mut statement = connection.prepare("SELECT root,identity FROM projects ORDER BY root")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let root = paths::path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        if !root.is_absolute() || root.parent().is_none() {
            return Err(Error::operation("invalid registered project root"));
        }
        consume(root, &row.get::<_, String>(1)?)?;
    }
    Ok(())
}

fn directory(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)
        .map_err(|e| Error::io("inspect uninstall directory", path, e))?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(Error::operation(format!(
            "uninstall target is not a real directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn entries(path: &Path) -> Result<fs::ReadDir> {
    fs::read_dir(path).map_err(|e| Error::io("inspect uninstall contents", path, e))
}

fn remove_file(path: &Path) -> Result<()> {
    fs::remove_file(path)
        .map_err(|e| Error::io("remove owned data; uninstall may be partial", path, e))
}

fn remove_directory(path: &Path) -> Result<()> {
    fs::remove_dir(path).map_err(|e| Error::io("finish owned directory removal", path, e))
}

fn regular(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(|e| Error::io("inspect uninstall file", path, e))?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(Error::operation(format!(
            "unknown data was preserved: {}",
            path.display()
        )));
    }
    Ok(())
}

fn history(root: &Path, identity: &str, remove: bool, cancel: &Cancellation) -> Result<()> {
    cancel.check()?;
    let canonical = fs::canonicalize(root).map_err(|e| {
        Error::io(
            "locate registered project; use its new location before uninstalling",
            root,
            e,
        )
    })?;
    if canonical != root {
        return Err(Error::operation(
            "registered project moved or became a symlink; use its actual location first",
        ));
    }
    let path = root.join(".kodstar");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io("inspect project history", &path, e)),
        _ => {}
    }
    directory(&path)?;
    let _lock = FileLock::try_acquire(&path.join("repository.lock"), false)?
        .ok_or_else(|| Error::operation(format!("project is in use: {}", root.display())))?;
    for entry in entries(&path)? {
        cancel.check()?;
        let entry = entry.map_err(|e| Error::caused_by("read history entry", e))?;
        if entry.file_name() == "checkout" {
            staging(&entry.path(), false, cancel)?;
        } else if entry
            .file_name()
            .to_str()
            .is_some_and(|n| HISTORY_FILES.contains(&n))
        {
            regular(&entry.path())?;
        } else {
            return Err(Error::operation(format!(
                "unknown history data was preserved: {}",
                entry.path().display()
            )));
        }
    }
    let database = path.join("repository.sqlite");
    regular(&database)?;
    let connection = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 65536)?;
    connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA cache_size=-256;")?;
    let app: u32 = connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if !crate::infrastructure::history::recognized_format(app, version) {
        return Err(Error::operation(
            "unknown history format; data was preserved",
        ));
    }
    let stored: String =
        connection.query_row("SELECT identity FROM repository", [], |r| r.get(0))?;
    if stored != identity {
        return Err(Error::operation(format!(
            "repository identity changed; data was preserved: {}",
            root.display()
        )));
    }
    let pending: bool =
        connection.query_row("SELECT EXISTS(SELECT 1 FROM checkout)", [], |r| r.get(0))?;
    if pending {
        return Err(Error::operation(
            "checkout recovery is pending; finish recovery before uninstalling",
        ));
    }
    drop(connection);
    if remove {
        for entry in entries(&path)? {
            cancel.check()?;
            let entry = entry.map_err(|e| Error::caused_by("read history removal entry", e))?;
            if entry.file_name() == "checkout" {
                staging(&entry.path(), true, cancel)?;
            } else if matches!(
                entry.file_name().to_str(),
                Some("repository.sqlite" | "repository.lock")
            ) {
                continue;
            } else if entry
                .file_name()
                .to_str()
                .is_some_and(|n| HISTORY_FILES.contains(&n))
            {
                regular(&entry.path())?;
                remove_file(&entry.path())?;
            } else {
                return Err(Error::operation(format!(
                    "history changed during uninstall; remaining data was preserved: {}",
                    entry.path().display()
                )));
            }
        }
        regular(&database)?;
        remove_file(&database)?;
        let lock = path.join("repository.lock");
        regular(&lock)?;
        remove_file(&lock)?;
        remove_directory(&path)?;
    }
    Ok(())
}

fn staging(path: &Path, remove: bool, cancel: &Cancellation) -> Result<()> {
    directory(path)?;
    for entry in entries(path)? {
        cancel.check()?;
        let entry = entry.map_err(|e| Error::caused_by("inspect checkout staging", e))?;
        let m = fs::symlink_metadata(entry.path())
            .map_err(|e| Error::io("inspect staging file", &entry.path(), e))?;
        if !hex_name(&entry.file_name().to_string_lossy())
            || (!m.is_file() && !m.file_type().is_symlink())
        {
            return Err(Error::operation(
                "unknown checkout staging data was preserved",
            ));
        }
        if remove {
            remove_file(&entry.path())?;
        }
    }
    if remove {
        remove_directory(path)?;
    }
    Ok(())
}

fn hex_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn home(store: &CacheStore, remove: bool, cancel: &Cancellation) -> Result<()> {
    for entry in entries(&store.home)? {
        cancel.check()?;
        let entry = entry.map_err(|e| Error::caused_by("read application home", e))?;
        let os_name = entry.file_name();
        let name = os_name
            .to_str()
            .ok_or_else(|| Error::operation("unknown application data was preserved"))?;
        if HOME_FILES.contains(&name) {
            regular(&entry.path())?;
            if remove && !matches!(name, "installation.lock" | "install.json" | "registry.lock") {
                remove_file(&entry.path())?;
            }
        } else if hex_name(name) {
            directory(&entry.path())?;
            let (root, _) = store.read_record(&entry.path())?;
            if remove {
                use crate::infrastructure::cache::removal::{self, Removal};
                if !matches!(
                    removal::remove(store, name, &root, None, false, cancel)?,
                    Removal::Removed
                ) {
                    return Err(Error::operation(
                        "cache changed or became active during uninstall; remaining data was preserved",
                    ));
                }
            } else {
                let _lease = FileLock::try_acquire(&entry.path().join("lease.lock"), false)?
                    .ok_or_else(|| Error::operation("analysis cache is in use"))?;
                if !inspection::inspect(&entry.path(), cancel, None)?.safe {
                    return Err(Error::operation(format!(
                        "unknown cache data was preserved: {}",
                        entry.path().display()
                    )));
                }
            }
        } else {
            return Err(Error::operation(format!(
                "unknown application data was preserved: {}",
                entry.path().display()
            )));
        }
    }
    Ok(())
}
