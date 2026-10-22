// Purpose: package의 공개 subpath·조건·source twin 파일 선택.
// Pattern: Simple Module.
// Usage: package 해석기가 식별한 manifest와 subpath를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/javascript/packages.rs, crates/kodstar-core/src/domain/resolve/path_patterns.rs.

use crate::application::analysis::resolve::indexed_paths::PathKind;
use crate::application::analysis::resolve::{FileContext, Lookup, indexed_paths};
use crate::domain::diagnostics::Result;
use crate::domain::resolve::path_patterns::match_pattern;
use crate::infrastructure::cache::paths::path_from_bytes;
use rusqlite::params;
use std::path::{Path, PathBuf};

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn package_pattern(
        &self,
        manifest: i64,
        kind: &str,
        raw: &str,
    ) -> Result<Option<String>> {
        let mut statement=self.connection.cached_prepare("SELECT name FROM bindings WHERE file_id=?1 AND kind=?2 GROUP BY name ORDER BY MIN(start_byte)")?;
        let records = statement.query_map(params![manifest, kind], |r| r.get::<_, String>(0))?;
        let mut best: Option<(String, u8, usize)> = None;
        for record in records {
            let pattern = record?;
            let Some(found) = match_pattern(raw, &pattern).filter(|m| m.rank < 2) else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|b| found.rank < b.1 || found.rank == b.1 && found.prefix > b.2)
            {
                best = Some((pattern, found.rank, found.prefix));
            }
        }
        Ok(best.map(|b| b.0))
    }

    pub(in crate::application::analysis::resolve) fn package_directory(
        &self,
        manifest: i64,
    ) -> Result<PathBuf> {
        let raw: Vec<u8> =
            self.connection
                .cached_row("SELECT path FROM files WHERE id=?1", [manifest], |r| {
                    r.get(0)
                })?;
        Ok(path_from_bytes(&raw)?
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf())
    }

    pub(in crate::application::analysis::resolve) fn package_options(
        &self,
        context: &FileContext,
        manifest: i64,
        kind: &str,
        pattern: &str,
        depth: usize,
    ) -> Result<()> {
        let native = context.path.components().any(|c| {
            c.as_os_str().to_str().is_some_and(|s| {
                matches!(s, "native" | "mobile" | "ios" | "android" | "react-native")
            })
        });
        self.connection.cached_execute(
            "DELETE FROM resolution_package_entries WHERE depth=?1",
            [depth],
        )?;
        self.connection.cached_execute("INSERT INTO resolution_package_entries
            SELECT ?1,replace(COALESCE(imported_name,''),'N','0'),start_byte,module_name,instr(COALESCE(imported_name,''),'N')>0 FROM bindings
            WHERE file_id=?2 AND kind=?3 AND name=?4 AND (?5 OR instr(COALESCE(imported_name,''),'N')=0)",params![depth,manifest,kind,pattern,native])?;
        Ok(())
    }

    pub(in crate::application::analysis::resolve) fn package_source(
        &mut self,
        directory: &Path,
        target: &str,
        strict: bool,
        depth: usize,
    ) -> Result<bool> {
        if strict && !target.starts_with("./") {
            return Ok(false);
        }
        let Some(path) = indexed_paths::normalize(directory, target) else {
            return Ok(false);
        };
        if strict && !path.starts_with(directory) {
            return Ok(false);
        }
        if let Some(twin) = source_twin(directory, &path)
            && (self.indexed_path(&twin, PathKind::JavaScript, depth)?
                || self.platform_sources(&twin, depth)?)
        {
            self.rule = "package_source_twin";
            return Ok(true);
        }
        if self.indexed_path(&path, PathKind::JavaScript, depth)? {
            return Ok(true);
        }
        self.platform_sources(&path, depth)
    }

    pub(in crate::application::analysis::resolve) fn package_exports(
        &mut self,
        context: &FileContext,
        manifest: i64,
        subpath: &str,
        depth: usize,
    ) -> Result<bool> {
        if subpath.split('/').any(|s| matches!(s, "..")) {
            return Ok(false);
        }
        let directory = self.package_directory(manifest)?;
        let exported:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND kind='package_exports_marker')",[manifest],|r|r.get(0))?;
        if exported {
            let Some(pattern) = self.package_pattern(manifest, "package_export", subpath)? else {
                return Ok(false);
            };
            self.package_options(context, manifest, "package_export", &pattern, depth)?;
            let captured = match_pattern(subpath, &pattern)
                .map(|m| m.captured)
                .unwrap_or("");
            let mut entries=self.connection.cached_prepare("SELECT target,native FROM resolution_package_entries WHERE depth=?1 ORDER BY priority,position")?;
            let records = entries.query_map([depth], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
            })?;
            for target in records {
                let (target, native) = target?;
                if self.package_source(
                    &directory,
                    &target.replacen('*', captured, 1),
                    true,
                    depth,
                )? {
                    if native {
                        self.platform_candidate();
                    }
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if subpath != "." {
            return self.package_source(&directory, subpath, true, depth);
        }
        let mut entries=self.connection.cached_prepare("SELECT module_name FROM bindings WHERE file_id=?1 AND kind='package_entry' ORDER BY CASE name WHEN 'source' THEN 0 WHEN 'module' THEN 1 WHEN 'main' THEN 2 ELSE 3 END,start_byte")?;
        let records = entries.query_map([manifest], |r| r.get::<_, String>(0))?;
        for target in records {
            if self.package_source(&directory, &target?, false, depth)? {
                return Ok(true);
            }
        }
        for target in ["./src/index", "./index"] {
            if self.package_source(&directory, target, false, depth)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(in crate::application::analysis::resolve) fn platform_candidate(&mut self) {
        self.candidate_only = true;
        self.reason = Some("platform-specific source requires explicit build conditions");
    }

    fn platform_sources(&mut self, path: &Path, depth: usize) -> Result<bool> {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            return Ok(false);
        };
        let extensions = path
            .extension()
            .and_then(|s| s.to_str())
            .map_or_else(|| vec!["ts", "tsx", "js", "jsx"], |s| vec![s]);
        let mut found = false;
        for platform in ["web", "native", "ios", "android"] {
            for extension in &extensions {
                found |= self.indexed_path(
                    &path.with_file_name(format!("{stem}.{platform}.{extension}")),
                    PathKind::JavaScript,
                    depth,
                )?;
            }
        }
        if found {
            self.platform_candidate();
        }
        Ok(found)
    }
}

fn source_twin(directory: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(directory).ok()?;
    let mut parts = relative.components();
    if !matches!(
        parts.next()?.as_os_str().to_str()?,
        "dist" | "build" | "target" | "out" | "dist-protected"
    ) {
        return None;
    }
    let rest = parts.as_path();
    let text = rest.to_str()?;
    let stem = [".d.ts", ".d.mts", ".d.cts", ".js", ".mjs", ".cjs"]
        .into_iter()
        .find_map(|suffix| text.strip_suffix(suffix))
        .unwrap_or(text);
    Some(directory.join("src").join(stem))
}
