// Purpose: 이력 수집에 필요한 디스크 파일·symlink의 ignore 순회.
// Pattern: Filesystem Adapter.
// Usage: history capture가 파일별 callback으로 소비.
// Related: crates/kodstar-core/src/application/history/capture.rs.

//! Streaming filesystem discovery. Missing scan regions are fatal rather than deletions.

use std::fs::{self, Metadata};
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};

pub(crate) struct Entry {
    pub relative: std::path::PathBuf,
    pub absolute: std::path::PathBuf,
    pub metadata: Metadata,
}

pub(crate) fn metadata_key(metadata: &Metadata) -> Result<(String, String)> {
    let modified = metadata
        .modified()
        .map_err(|error| Error::caused_by("read file modification time", error))?
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos().to_string())
        .unwrap_or_else(|error| format!("-{}", error.duration().as_nanos()));
    let changed = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", metadata.ctime(), metadata.ctime_nsec())
    };
    Ok((modified, changed))
}

pub(crate) fn walk(
    root: &Path,
    cache_home: &Path,
    excludes: &[String],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Entry) -> Result<()>,
) -> Result<()> {
    let excluded: Vec<_> = excludes
        .iter()
        .map(|pattern| globset::Glob::new(pattern).map(|glob| glob.compile_matcher()))
        .collect::<std::result::Result<_, _>>()
        .map_err(|error| Error::caused_by("compile excluded paths", error))?;
    let skip_cache = cache_home.to_path_buf();
    let root_filter = root.to_path_buf();
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(false)
        .require_git(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            if entry.path().starts_with(&skip_cache) {
                return false;
            }
            let name = entry.file_name().to_str().unwrap_or("");
            if matches!(name, ".git" | ".kodstar") {
                return false;
            }
            if entry.file_type().is_some_and(|kind| kind.is_dir())
                && matches!(
                    name,
                    ".git"
                        | ".kodstar"
                        | "target"
                        | "node_modules"
                        | ".venv"
                        | "venv"
                        | "__pycache__"
                        | "dist"
                        | "build"
                        | "vendor"
                )
            {
                return false;
            }
            let relative = entry
                .path()
                .strip_prefix(&root_filter)
                .unwrap_or(entry.path());
            !excluded.iter().any(|pattern| pattern.is_match(relative))
        });
    for entry in builder.build() {
        cancel.check()?;
        let entry = entry.map_err(|error| {
            Error::caused_by(
                "project scan incomplete; previous generation preserved",
                error,
            )
        })?;
        if !entry
            .file_type()
            .is_some_and(|kind| kind.is_file() || kind.is_symlink())
        {
            continue;
        }
        let absolute = entry.path().to_path_buf();
        let relative = absolute
            .strip_prefix(root)
            .map_err(|_| Error::operation("scanner escaped project root"))?
            .to_path_buf();
        let metadata = fs::symlink_metadata(&absolute)
            .map_err(|error| Error::io("read file metadata", &absolute, error))?;
        emit(Entry {
            relative,
            absolute,
            metadata,
        })?;
    }
    Ok(())
}
