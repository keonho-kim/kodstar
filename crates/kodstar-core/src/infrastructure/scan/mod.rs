// Purpose: ignore 순회와 크기가 제한된 안정적 파일 읽기.
// Pattern: Filesystem Adapter.
// Usage: discovery가 파일별 callback으로 소비.
// Related: crates/kodstar-core/src/application/analysis/discovery.rs.

//! Streaming filesystem discovery. Missing scan regions are fatal rather than deletions.

use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::languages::Language;
use crate::infrastructure::languages;

pub(crate) struct Entry {
    pub relative: PathBuf,
    pub absolute: PathBuf,
    pub metadata: Metadata,
    pub language: Option<Language>,
    pub auxiliary: bool,
    pub detect_prefix: bool,
}

pub(crate) fn metadata_key(metadata: &Metadata) -> Result<(String, String)> {
    let modified = metadata
        .modified()
        .map_err(|error| Error::caused_by("read file modification time", error))?
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos().to_string())
        .unwrap_or_else(|error| format!("-{}", error.duration().as_nanos()));
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", metadata.ctime(), metadata.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = String::new();
    Ok((modified, changed))
}

pub(crate) fn walk(
    root: &Path,
    cache_home: &Path,
    excludes: &[String],
    overrides: &[(globset::GlobMatcher, Language)],
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
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let absolute = entry.path().to_path_buf();
        let relative = absolute
            .strip_prefix(root)
            .map_err(|_| Error::operation("scanner escaped project root"))?
            .to_path_buf();
        let metadata = fs::metadata(&absolute)
            .map_err(|error| Error::io("read file metadata", &absolute, error))?;
        let override_language = overrides
            .iter()
            .find(|(pattern, _)| pattern.is_match(&relative))
            .map(|(_, language)| *language);
        let language = override_language.or_else(|| languages::detect(&relative, &[]));
        let detect_prefix = override_language.is_none()
            && (relative.extension().is_some_and(|value| value == "h")
                || relative.extension().is_none() && language.is_none());
        let auxiliary = super::formats::is_auxiliary(&relative);
        emit(Entry {
            relative,
            absolute,
            metadata,
            language,
            auxiliary,
            detect_prefix,
        })?;
    }
    Ok(())
}

pub(crate) fn detect_from_prefix(entry: &Entry) -> Result<Option<Language>> {
    let mut prefix = Vec::new();
    File::open(&entry.absolute)
        .and_then(|file| file.take(4096).read_to_end(&mut prefix))
        .map_err(|error| Error::io("read language detection prefix", &entry.absolute, error))?;
    Ok(languages::detect(&entry.relative, &prefix))
}

pub(crate) fn read_stable(entry: &Entry, max_bytes: usize) -> Result<(Vec<u8>, Metadata)> {
    for _ in 0..3 {
        let file = File::open(&entry.absolute)
            .map_err(|error| Error::io("open analysis input", &entry.absolute, error))?;
        let before = file
            .metadata()
            .map_err(|error| Error::io("inspect analysis input", &entry.absolute, error))?;
        let mut bytes = Vec::with_capacity(
            usize::try_from(before.len())
                .unwrap_or(max_bytes)
                .min(max_bytes),
        );
        file.take(max_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| Error::io("read analysis input", &entry.absolute, error))?;
        let after = fs::metadata(&entry.absolute)
            .map_err(|error| Error::io("recheck analysis input", &entry.absolute, error))?;
        if before.len() == after.len() && metadata_key(&before)? == metadata_key(&after)? {
            return Ok((bytes, after));
        }
    }
    Err(Error::operation(format!(
        "file kept changing during analysis: {}; previous generation preserved",
        entry.relative.display()
    )))
}
