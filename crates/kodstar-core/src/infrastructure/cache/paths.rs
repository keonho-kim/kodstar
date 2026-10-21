// Purpose: 플랫폼 경로·canonical 포함 관계·cache 식별.
// Pattern: Simple Module.
// Usage: cache·scan·index 경계가 호출.
// Related: crates/kodstar-core/src/infrastructure/cache/mod.rs.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::diagnostics::{Error, Result};
use crate::domain::source::PathEncoding;

#[cfg(unix)]
pub(crate) const PLATFORM: &str = "unix";
#[cfg(windows)]
pub(crate) const PLATFORM: &str = "windows";

#[cfg(unix)]
pub(crate) fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
pub(crate) fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(unix)]
pub(crate) fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(windows)]
pub(crate) fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    if !bytes.len().is_multiple_of(2) {
        return Err(Error::operation("invalid UTF-16 path bytes in index"));
    }
    let words: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Ok(PathBuf::from(OsString::from_wide(&words)))
}

pub(crate) fn cache_key(root: &Path) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"kodstar-path-v1\0");
    hasher.update(PLATFORM.as_bytes());
    hasher.update(&[0]);
    hasher.update(&path_bytes(root));
    hasher.finalize().to_hex().to_string()
}

pub(crate) fn encoding(path: &Path) -> Option<PathEncoding> {
    path.to_str().is_none().then(|| PathEncoding {
        platform: PLATFORM,
        bytes_base64: base64(&path_bytes(path)),
    })
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[usize::from(a >> 2)] as char);
        output.push(ALPHABET[usize::from(((a & 3) << 4) | (b >> 4))] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[usize::from(((b & 15) << 2) | (c >> 6))] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[usize::from(c & 63)] as char
        } else {
            '='
        });
    }
    output
}

pub(crate) fn canonical_directory(path: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(path)
        .map_err(|error| Error::io("resolve project directory", path, error))?;
    if !root.is_dir() {
        return Err(Error::argument(format!(
            "project root is not a directory: {}",
            path.display()
        )));
    }
    Ok(root)
}

pub(crate) fn relative_source(root: &Path, cwd: &Path, input: &Path) -> Result<PathBuf> {
    let absolute = if input.is_absolute() {
        input.to_path_buf()
    } else {
        cwd.join(input)
    };
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    while !existing
        .try_exists()
        .map_err(|error| Error::io("inspect source path", existing, error))?
    {
        missing.push(
            existing
                .file_name()
                .ok_or_else(|| Error::argument("source path has no existing ancestor"))?,
        );
        existing = existing
            .parent()
            .ok_or_else(|| Error::argument("source path has no existing ancestor"))?;
    }
    let mut resolved = fs::canonicalize(existing)
        .map_err(|error| Error::io("resolve source path", existing, error))?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    resolved
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| {
            Error::argument(format!(
                "source path is outside project root: {}",
                input.display()
            ))
        })
}
