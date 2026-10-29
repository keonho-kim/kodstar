// Purpose: 수집과 감시가 공유하는 확실한 제외 경로 판정.
// Pattern: Specification.
// Usage: scan 순회와 watch 이벤트 필터가 root·cache·명시 패턴을 전달.
// Related: crates/kodstar-core/src/infrastructure/scan/mod.rs, crates/kodstar-core/src/infrastructure/watch.rs.

use crate::domain::diagnostics::{Error, Result};
use std::path::{Path, PathBuf};

pub(crate) struct Policy {
    root: PathBuf,
    cache: PathBuf,
    excluded: Vec<globset::GlobMatcher>,
}

impl Policy {
    pub fn new(root: &Path, cache: &Path, excludes: &[String]) -> Result<Self> {
        let excluded = excludes
            .iter()
            .map(|p| globset::Glob::new(p).map(|p| p.compile_matcher()))
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| Error::caused_by("compile excluded paths", e))?;
        Ok(Self {
            root: root.into(),
            cache: cache.into(),
            excluded,
        })
    }
    pub fn accepts(&self, path: &Path, directory: bool) -> bool {
        if !self.accepts_entry(path, directory) {
            return false;
        }
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return true;
        };
        relative
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .filter(|part| !part.as_os_str().is_empty())
            .all(|part| !self.excludes(part, true))
    }

    pub fn accepts_entry(&self, path: &Path, directory: bool) -> bool {
        if path == self.root {
            return true;
        }
        if path.starts_with(&self.cache) {
            return false;
        }
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return true;
        };
        !self.excludes(relative, directory)
    }

    fn excludes(&self, relative: &Path, directory: bool) -> bool {
        let name = relative.file_name().and_then(|n| n.to_str()).unwrap_or("");
        matches!(name, ".git" | ".kodstar")
            || directory
                && matches!(
                    name,
                    "target"
                        | "node_modules"
                        | ".venv"
                        | "venv"
                        | "__pycache__"
                        | "dist"
                        | "build"
                        | "vendor"
                )
            || self
                .excluded
                .iter()
                .any(|pattern| pattern.is_match(relative))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_names_unknown_paths_and_excluded_ancestors_remain_distinct() -> Result<()> {
        let p = Policy::new(
            Path::new("/project"),
            Path::new("/cache"),
            &["generated".into()],
        )?;
        assert!(p.accepts(Path::new("/project/target"), false));
        assert!(!p.accepts(Path::new("/project/target"), true));
        for path in [
            "/project/target/a",
            "/project/generated/a",
            "/project/.git",
            "/cache/a",
        ] {
            assert!(!p.accepts(Path::new(path), false), "{path}");
        }
        assert!(p.accepts(Path::new("/project/src/a.rs"), false));
        assert!(p.accepts(Path::new("/elsewhere"), false));
        Ok(())
    }
}
