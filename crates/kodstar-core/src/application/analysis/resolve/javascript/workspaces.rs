// Purpose: 명시적 workspace·로컬 의존성에서 접근 가능한 package 선택.
// Pattern: Simple Module.
// Usage: package 경로 해석이 소비자와 package 이름을 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/javascript/packages.rs, crates/kodstar-core/src/infrastructure/formats/auxiliary/packages.rs.

use crate::application::analysis::resolve::{FileContext, Lookup, indexed_paths};
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::infrastructure::cache::paths::{path_bytes, path_from_bytes};
use rusqlite::{OptionalExtension, params};
use std::path::{Path, PathBuf};

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn nearest_package(
        &self,
        context: &FileContext,
    ) -> Result<Option<(i64, PathBuf)>> {
        for directory in context
            .path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .ancestors()
        {
            if let Some(id) = self.package_at(directory)? {
                return Ok(Some((id, directory.to_path_buf())));
            }
        }
        Ok(None)
    }

    fn package_at(&self, directory: &Path) -> Result<Option<i64>> {
        Ok(self.connection.query_row("SELECT f.id FROM files f WHERE f.path=?1 AND f.status='auxiliary' AND EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=f.id AND b.kind='package_manifest')",[path_bytes(&directory.join("package.json"))],|r|r.get(0)).optional()?)
    }

    pub(in crate::application::analysis::resolve) fn select_packages(
        &self,
        context: &FileContext,
        name: &str,
        depth: usize,
    ) -> Result<bool> {
        self.connection.execute(
            "DELETE FROM resolution_package_candidates WHERE depth=?1",
            [depth],
        )?;
        let mut declared = false;
        if let Some((consumer, directory)) = self.nearest_package(context)? {
            self.connection.execute("INSERT OR IGNORE INTO resolution_package_candidates SELECT ?1,f.id FROM files f WHERE f.id=?2 AND f.declared_module=?3 AND EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=f.id AND b.kind='package_exports_marker')",params![depth,consumer,name])?;
            let mut links=self.connection.prepare("SELECT module_name FROM bindings WHERE file_id=?1 AND kind='package_link' AND name=?2 ORDER BY id")?;
            let links = links.query_map(params![consumer, name], |r| r.get::<_, String>(0))?;
            for raw in links {
                declared = true;
                if let Some(directory) = indexed_paths::normalize(&directory, &raw?)
                    && let Some(manifest) = self.package_at(&directory)?
                {
                    self.connection.execute(
                        "INSERT OR IGNORE INTO resolution_package_candidates VALUES(?1,?2)",
                        params![depth, manifest],
                    )?;
                }
            }
        }
        for directory in context
            .path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .ancestors()
        {
            let mut root = None;
            for manifest in ["pnpm-workspace.yaml", "pnpm-workspace.yml", "package.json"] {
                root=self.connection.query_row("SELECT f.id FROM files f WHERE f.path=?1 AND EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=f.id AND b.kind='workspace_root')",[path_bytes(&directory.join(manifest))],|r|r.get::<_,i64>(0)).optional()?;
                if root.is_some() {
                    break;
                }
            }
            if let Some(root) = root {
                self.load_workspace(root, directory)?;
                self.connection.execute("INSERT OR IGNORE INTO resolution_package_candidates SELECT ?1,manifest FROM resolution_workspace_packages WHERE root=?2 AND name=?3",params![depth,root,name])?;
                break;
            }
        }
        let found: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_package_candidates WHERE depth=?1)",
            [depth],
            |r| r.get(0),
        )?;
        Ok(declared || found)
    }

    fn load_workspace(&self, root: i64, directory: &Path) -> Result<()> {
        if self.connection.execute(
            "INSERT OR IGNORE INTO resolution_workspace_seen VALUES(?1)",
            [root],
        )? == 0
        {
            return Ok(());
        }
        let mut patterns=self.connection.prepare("SELECT name FROM bindings WHERE file_id=?1 AND kind='workspace_glob' ORDER BY start_byte,id")?;
        let patterns = patterns.query_map([root], |r| r.get::<_, String>(0))?;
        for pattern in patterns {
            let pattern = pattern?;
            if pattern.len() > 4096 {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "workspace pattern exceeds 4096 bytes",
                ));
            }
            let (exclude, pattern) = pattern
                .strip_prefix('!')
                .map_or((false, pattern.as_str()), |p| (true, p));
            let matcher = globset::GlobBuilder::new(pattern.trim_start_matches("./"))
                .literal_separator(true)
                .build()
                .map_err(|e| Error::caused_by("invalid workspace pattern", e))?
                .compile_matcher();
            let mut packages=self.connection.prepare("SELECT f.id,f.path,f.declared_module FROM files f WHERE f.status='auxiliary' AND f.declared_module<>'' AND EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=f.id AND b.kind='package_manifest') ORDER BY f.id")?;
            let packages = packages.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for package in packages {
                let (manifest, path, name) = package?;
                let path = path_from_bytes(&path)?;
                let Some(relative) = path.parent().and_then(|p| p.strip_prefix(directory).ok())
                else {
                    continue;
                };
                if !(matches!(pattern, "." | "./") && relative.as_os_str().is_empty()
                    || matcher.is_match(relative))
                {
                    continue;
                }
                if exclude {
                    self.connection.execute(
                        "INSERT OR IGNORE INTO resolution_workspace_excluded VALUES(?1,?2)",
                        params![root, manifest],
                    )?;
                } else {
                    self.connection.execute(
                        "INSERT OR IGNORE INTO resolution_workspace_packages VALUES(?1,?2,?3)",
                        params![root, name, manifest],
                    )?;
                }
            }
        }
        self.connection.execute("DELETE FROM resolution_workspace_packages WHERE root=?1 AND manifest IN (SELECT manifest FROM resolution_workspace_excluded WHERE root=?1)",[root])?;
        Ok(())
    }
}
