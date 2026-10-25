// Purpose: 인덱스에 포함된 Go manifest와 최근접 workspace 구성원 선택.
// Pattern: Simple Module.
// Usage: Go 경로 해석이 현재 파일의 정적 module 문맥을 요청.
// Related: crates/kodstar-core/src/application/analysis/resolve/go/mod.rs, crates/kodstar-core/src/application/analysis/resolve/path_caches.sql.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::{path_bytes, path_from_bytes};
use rusqlite::{OptionalExtension, params};
use std::path::{Path, PathBuf};

pub(in crate::application::analysis::resolve) struct Manifest {
    pub id: i64,
    pub path: PathBuf,
    pub package: String,
    pub module: String,
}

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, Vec<u8>, String, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn go_manifest(
        &self,
        path: &Path,
    ) -> Result<Option<Manifest>> {
        let row=self.connection.cached_row("SELECT id,path,package,declared_module FROM files WHERE path=?1 AND status='auxiliary'",[path_bytes(path)],read).optional()?;
        row.map(|(id, path, package, module)| {
            Ok(Manifest {
                id,
                path: path_from_bytes(&path)?,
                package,
                module,
            })
        })
        .transpose()
    }

    pub(in crate::application::analysis::resolve) fn go_own_manifest(
        &self,
        c: &FileContext,
    ) -> Result<Option<Manifest>> {
        let row=self.connection.cached_row("SELECT id,path,package,declared_module FROM files WHERE package_id=(SELECT id FROM atoms WHERE value=?1) AND status_id=(SELECT id FROM atoms WHERE value='auxiliary') AND declared_module<>'' AND (display_path='go.mod' OR display_path LIKE '%/go.mod') LIMIT 1",[&c.package],read).optional()?;
        row.map(|(id, path, package, module)| {
            Ok(Manifest {
                id,
                path: path_from_bytes(&path)?,
                package,
                module,
            })
        })
        .transpose()
    }

    pub(in crate::application::analysis::resolve) fn go_workspace(
        &self,
        c: &FileContext,
        own: &Manifest,
    ) -> Result<Option<i64>> {
        let cached: Option<Option<i64>> = self
            .connection
            .cached_row(
                "SELECT workspace FROM go_workspace_selection WHERE file=?1",
                [c.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(cached) = cached {
            return Ok(cached);
        }
        let mut selected = None;
        for parent in c.path.parent().unwrap_or_else(|| Path::new("")).ancestors() {
            let Some(workspace) = self.go_manifest(&parent.join("go.work"))? else {
                continue;
            };
            self.go_workspace_members(&workspace)?;
            let member:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM go_workspace_members WHERE workspace=?1 AND manifest=?2)",params![workspace.id,own.id],|r|r.get(0))?;
            if member {
                selected = Some(workspace.id);
            }
            break;
        }
        self.connection.cached_execute(
            "INSERT INTO go_workspace_selection VALUES(?1,?2)",
            params![c.id, selected],
        )?;
        Ok(selected)
    }

    fn go_workspace_members(&self, workspace: &Manifest) -> Result<()> {
        let ready: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM go_workspace_ready WHERE workspace=?1)",
            [workspace.id],
            |r| r.get(0),
        )?;
        if ready {
            return Ok(());
        }
        let parent = workspace.path.parent().unwrap_or_else(|| Path::new(""));
        let mut statement=self.connection.cached_prepare("SELECT module_name FROM bindings WHERE file_id=?1 AND kind='go_workspace_use' ORDER BY id")?;
        let paths = statement.query_map([workspace.id], |r| r.get::<_, String>(0))?;
        for path in paths {
            if let Some(directory) =
                crate::application::analysis::resolve::indexed_paths::normalize(parent, &path?)
                && let Some(manifest) = self.go_manifest(&directory.join("go.mod"))?
                && !manifest.module.is_empty()
            {
                self.connection.cached_execute(
                    "INSERT OR IGNORE INTO go_workspace_members VALUES(?1,?2)",
                    params![workspace.id, manifest.id],
                )?;
            }
        }
        self.connection
            .cached_execute("INSERT INTO go_workspace_ready VALUES(?1)", [workspace.id])?;
        Ok(())
    }
}
