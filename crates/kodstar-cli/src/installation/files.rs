// Purpose: 설치 파일의 제한된 읽기·내용 식별·원자적 교체.
// Pattern: Filesystem Adapter.
// Usage: installation receipt와 shell 설정 갱신이 호출.
// Related: crates/kodstar-cli/src/installation/mod.rs, crates/kodstar-cli/src/installation/shell.rs.

use kodstar_core::{Error, ErrorKind, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(super) fn failure(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Operation, message)
}

pub(super) fn regular(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)
        .map_err(|e| Error::caused_by(format!("inspect {}", path.display()), e))?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(failure(format!(
            "expected a regular file; preserved {}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::caused_by("inspect installation file", e)),
        _ => regular(path)?,
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|e| Error::caused_by("read installation file", e))?;
    if bytes.len() > 1024 * 1024 {
        return Err(failure(
            "installation or shell file exceeds 1 MiB; data was preserved",
        ));
    }
    Ok(Some(bytes))
}

pub(super) fn digest(path: &Path) -> Result<String> {
    regular(path)?;
    let mut file = File::open(path).map_err(|e| Error::caused_by("open installed binary", e))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| Error::caused_by("hash installed binary", e))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub(super) fn directory(path: &Path, created: &mut Vec<PathBuf>) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => return physical_directory(path),
        Ok(_) => {
            return Err(failure(format!(
                "expected a real directory: {}",
                path.display()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::caused_by("inspect installation directory", e)),
    }
    let parent = path
        .parent()
        .ok_or_else(|| failure("installation directory has no parent"))?;
    directory(parent, created)?;
    fs::create_dir(path).map_err(|e| Error::caused_by(format!("create {}", path.display()), e))?;
    created.push(path.to_path_buf());
    Ok(())
}

pub(super) fn plan_directory(path: &Path, planned: &mut Vec<PathBuf>) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => return physical_directory(path),
        Ok(_) => {
            return Err(failure(format!(
                "expected a real directory: {}",
                path.display()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::caused_by("inspect installation directory", e)),
    }
    plan_directory(
        path.parent()
            .ok_or_else(|| failure("installation directory has no parent"))?,
        planned,
    )?;
    if !planned.iter().any(|p| p == path) {
        planned.push(path.to_path_buf());
    }
    Ok(())
}

fn physical_directory(path: &Path) -> Result<()> {
    if fs::canonicalize(path).map_err(|e| Error::caused_by("resolve installation directory", e))?
        != path
    {
        return Err(failure(format!(
            "installation directory contains an alias; preserved {}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn atomic(
    path: &Path,
    write: &mut dyn FnMut(&mut File) -> Result<()>,
    mode: u32,
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| failure("installation file has no parent"))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| Error::caused_by("read installation clock", e))?
        .as_nanos();
    let temporary = parent.join(format!(".kodstar-{}-{nonce}.tmp", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|e| Error::caused_by("create installation staging", e))?;
    let result = (|| {
        write(&mut file)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(mode))
                .map_err(|e| Error::caused_by("set installation permissions", e))?;
        }
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|e| Error::caused_by("persist installation", e))?;
        fs::rename(&temporary, path).map_err(|e| Error::caused_by("publish installation", e))?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| Error::caused_by("persist installation directory", e))?;
        Ok(())
    })();
    if result.is_err() && temporary.exists() {
        fs::remove_file(&temporary)
            .map_err(|e| Error::caused_by("clean installation staging", e))?;
    }
    result
}

pub(super) fn write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    atomic(
        path,
        &mut |file| {
            file.write_all(bytes)
                .map_err(|e| Error::caused_by("write installation file", e))
        },
        mode,
    )
}
