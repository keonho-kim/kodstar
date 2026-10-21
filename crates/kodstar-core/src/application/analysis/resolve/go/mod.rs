// Purpose: 명시적 Go workspace·현재 module·로컬 replacement에서 import 경로 선택.
// Pattern: Simple Module.
// Usage: module_files가 Go import를 인덱스 내부에서 해석.
// Related: crates/kodstar-core/src/application/analysis/resolve/go/manifests.rs, crates/kodstar-core/src/infrastructure/formats/auxiliary/go.rs.

pub(super) mod interfaces;
pub(super) mod manifests;

use crate::application::analysis::resolve::modules::Location;
use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::path_from_bytes;
use rusqlite::{OptionalExtension, params};
use std::path::Path;

struct Replacement {
    manifest_path: Vec<u8>,
    module: String,
    version: Option<String>,
    target: String,
    target_version: Option<String>,
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn go_paths(
        &mut self,
        c: &FileContext,
        raw: &str,
        depth: usize,
    ) -> Result<Location> {
        let Some(own) = self.go_own_manifest(c)? else {
            return Ok(Location::Unmapped);
        };
        let workspace = self.go_workspace(c, &own)?;
        let mut modules = self.connection.prepare(
            "SELECT m.package,m.declared_module
            FROM files m
            WHERE (m.id=?1
            OR m.id IN (SELECT manifest
            FROM go_workspace_members
            WHERE workspace=?2))
            AND (?3=m.declared_module
            OR substr(?3,1,length(m.declared_module)+1)=m.declared_module||'/')
            AND length(m.declared_module)=(SELECT MAX(length(n.declared_module))
            FROM files n
            WHERE (n.id=?1
            OR n.id IN (SELECT manifest
            FROM go_workspace_members
            WHERE workspace=?2))
            AND (?3=n.declared_module
            OR substr(?3,1,length(n.declared_module)+1)=n.declared_module||'/'))",
        )?;
        let rows = modules.query_map(params![own.id, workspace, raw], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut count = 0;
        for row in rows {
            let (package, module) = row?;
            count += 1;
            self.go_package_files(
                &package,
                raw.strip_prefix(&module)
                    .unwrap_or("")
                    .trim_start_matches('/'),
                depth,
            )?;
        }
        if count > 0 {
            if count > 1 {
                self.candidate_only = true;
                self.reason = Some("multiple workspace modules declare the same import prefix");
            }
            return Ok(Location::Project);
        }
        let replacement=self.connection.query_row("SELECT f.path,b.name,b.imported_name,b.module_name,b.type_name
            FROM bindings b JOIN files f ON f.id=b.file_id
            WHERE b.kind='go_replace' AND (b.file_id=?1 OR b.file_id=?2)
              AND (?3=b.name OR substr(?3,1,length(b.name)+1)=b.name||'/')
              AND EXISTS(SELECT 1 FROM bindings r WHERE r.kind='go_require' AND r.name=b.name
                AND (r.file_id=?1 OR r.file_id IN (SELECT manifest FROM go_workspace_members WHERE workspace=?2))
                AND (b.imported_name IS NULL OR b.imported_name=r.module_name))
            ORDER BY b.file_id=?2 DESC,length(b.name) DESC,b.imported_name IS NOT NULL DESC,b.id DESC LIMIT 1",
            params![own.id,workspace,raw],|r|Ok(Replacement {manifest_path:r.get(0)?,module:r.get(1)?,version:r.get(2)?,target:r.get(3)?,target_version:r.get(4)?})).optional()?;
        let Some(Replacement {
            manifest_path,
            module,
            version,
            target,
            target_version,
        }) = replacement
        else {
            return Ok(Location::Unmapped);
        };
        let suffix = raw
            .strip_prefix(&module)
            .unwrap_or("")
            .trim_start_matches('/');
        if version.is_some() {
            self.candidate_only = true;
            self.reason = Some(
                "version-specific replacement matches a declared requirement; full MVS selection is not evaluated",
            );
        }
        self.dependency("go_replacement", &module, true)?;
        if target_version.is_some() {
            return Ok(Location::External(if suffix.is_empty() {
                target
            } else {
                format!("{target}/{suffix}")
            }));
        }
        let path = path_from_bytes(&manifest_path)?;
        let parent = path.parent().unwrap_or_else(|| Path::new(""));
        let Some(directory) =
            crate::application::analysis::resolve::indexed_paths::normalize(parent, &target)
        else {
            self.reason = Some("Go replacement is outside the indexed root");
            return Ok(Location::Project);
        };
        if let Some(manifest) = self.go_manifest(&directory.join("go.mod"))? {
            if manifest.module == module {
                self.go_package_files(&manifest.package, suffix, depth)?;
            } else {
                self.reason =
                    Some("replacement module declaration does not match the replaced path");
            }
        } else {
            self.reason = Some("local Go replacement manifest is not indexed");
        }
        Ok(Location::Project)
    }

    fn go_package_files(&self, package: &str, module: &str, depth: usize) -> Result<()> {
        self.connection.execute("INSERT OR IGNORE INTO resolution_modules SELECT ?3,id FROM files WHERE language='go' AND package=?1 AND module=?2 AND status='analyzed'",params![package,module,depth])?;
        Ok(())
    }
}
