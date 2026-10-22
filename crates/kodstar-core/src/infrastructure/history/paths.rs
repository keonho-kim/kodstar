// Purpose: 이력의 손실 없는 상대 경로와 파일시스템 경계 검사.
// Pattern: Boundary Parser.
// Usage: capture·checkout·저장소 생성이 경로를 확인.
// Related: crates/kodstar-core/src/infrastructure/cache/paths.rs.

use crate::domain::diagnostics::{Error, Result};
use std::path::{Component, Path, PathBuf};

pub(crate) fn relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path
            .components()
            .next()
            .is_some_and(|c| c.as_os_str() == ".kodstar" || c.as_os_str() == ".git")
    {
        return Err(Error::argument(
            "expected a project-relative path outside internal storage",
        ));
    }
    Ok(())
}

pub(crate) fn from_input(root: &Path, cwd: &Path, input: &Path) -> Result<PathBuf> {
    let absolute = if input.is_absolute() {
        input.to_path_buf()
    } else {
        cwd.join(input)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(Error::argument("path escapes project root"));
                }
            }
            _ => normalized.push(component),
        }
    }
    let path = normalized
        .strip_prefix(root)
        .map_err(|_| Error::argument("path is outside project root"))?
        .to_path_buf();
    relative(&path)?;
    Ok(path)
}

pub(crate) fn destination(root: &Path, path: &Path) -> Result<PathBuf> {
    relative(path)?;
    let mut current = root.to_path_buf();
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    for component in parent.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(Error::operation("checkout parent is not a real directory"));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io("inspect checkout parent", &current, e)),
        }
    }
    Ok(root.join(path))
}

pub(crate) fn real_directory(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| Error::io("inspect history directory", path, e))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::operation(
            "history directory must be a real directory",
        ));
    }
    Ok(())
}

pub(crate) fn regular_database(path: &Path) -> Result<()> {
    for candidate in [
        path.to_path_buf(),
        path.with_file_name("repository.sqlite-wal"),
        path.with_file_name("repository.sqlite-shm"),
    ] {
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                return Err(Error::operation("history database must be a regular file"));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io("inspect history database", &candidate, e)),
        }
    }
    Ok(())
}

pub(crate) fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}
