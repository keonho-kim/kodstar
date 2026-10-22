// Purpose: JS/TS alias 우선순위·fallback·baseUrl을 인덱스 경로로 해석.
// Pattern: Simple Module.
// Usage: module_files가 비상대 JS/TS specifier를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/javascript/configurations.rs, crates/kodstar-core/src/application/analysis/resolve/indexed_paths.rs.

pub(super) mod configurations;
pub(super) mod packages;
pub(super) mod targets;
pub(super) mod workspaces;

use crate::application::analysis::resolve::indexed_paths::PathKind;
use crate::application::analysis::resolve::{FileContext, Lookup, indexed_paths};
use crate::domain::diagnostics::{Error, Result};
use crate::domain::resolve::path_patterns::match_pattern;
use crate::infrastructure::cache::paths::path_from_bytes;
use rusqlite::{OptionalExtension, params};
use std::path::Path;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn javascript_paths(
        &mut self,
        context: &FileContext,
        raw: &str,
        depth: usize,
    ) -> Result<bool> {
        let Some(root) = self.javascript_configuration(context)? else {
            return Ok(false);
        };
        self.load_javascript_configuration(root, root, 0)?;
        let mut best: Option<(String, i64, u8, usize)> = None;
        let mut patterns=self.connection.cached_prepare("SELECT pattern,configuration FROM resolution_path_patterns WHERE root=?1 ORDER BY rowid")?;
        let records = patterns.query_map([root], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for record in records {
            let (pattern, configuration) = record?;
            let Some(found) = match_pattern(raw, &pattern) else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|b| found.rank < b.2 || found.rank == b.2 && found.prefix > b.3)
            {
                best = Some((pattern, configuration, found.rank, found.prefix));
            }
        }
        self.dependency("configuration", &root.to_string(), best.is_some())?;
        if let Some((pattern, configuration, _, _)) = best {
            let Some(found) = match_pattern(raw, &pattern) else {
                return Err(Error::operation(
                    "selected configuration pattern no longer matches",
                ));
            };
            let mut targets=self.connection.cached_prepare("SELECT b.module_name,b.imported_name,f.path FROM bindings b JOIN files f ON f.id=b.file_id
                WHERE b.file_id=?1 AND b.kind='path_alias' AND b.name=?2
                  AND b.start_byte>(SELECT COALESCE(MAX(d.start_byte),-1) FROM bindings d WHERE d.file_id=b.file_id AND d.kind='path_alias_start' AND d.name=b.name)
                ORDER BY b.start_byte,b.id")?;
            let targets = targets.query_map(params![configuration, pattern], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?;
            for target in targets {
                let (target, base, file) = target?;
                let file = path_from_bytes(&file)?;
                let Some(base) = indexed_paths::normalize(
                    file.parent().unwrap_or_else(|| Path::new("")),
                    base.as_deref().unwrap_or("."),
                ) else {
                    continue;
                };
                let target = if found.wildcard && !found.captured.is_empty() {
                    target.replacen('*', found.captured, 1)
                } else if found.wildcard || found.captured.is_empty() {
                    target
                } else {
                    format!("{target}/{}", found.captured)
                };
                if let Some(path) = indexed_paths::normalize(&base, &target)
                    && self.indexed_path(&path, PathKind::JavaScript, depth)?
                {
                    return Ok(true);
                }
            }
            return Ok(true);
        }
        let base:Option<(String,Vec<u8>)>=self.connection.cached_row("SELECT b.module_name,f.path FROM bindings b JOIN files f ON f.id=b.file_id WHERE b.file_id=?1 AND b.kind='base_url' ORDER BY b.id DESC LIMIT 1",[root],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((base, file)) = base {
            let file = path_from_bytes(&file)?;
            if let Some(base) =
                indexed_paths::normalize(file.parent().unwrap_or_else(|| Path::new("")), &base)
                && let Some(path) = indexed_paths::normalize(&base, raw)
            {
                return self.indexed_path(&path, PathKind::JavaScript, depth);
            }
        }
        Ok(false)
    }
}
