// Purpose: Python wildcard export 목록과 모듈의 실제 lexical 재수출 바인딩 해석.
// Pattern: Simple Module.
// Usage: module lookup이 __all__ 필터와 Python 이름 선택을 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/python/exports.rs, crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::{OptionalExtension, params};

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn python_star_files(
        &mut self,
        name: &str,
        depth: usize,
    ) -> Result<()> {
        self.connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS resolution_python_exports(depth INTEGER,id INTEGER,PRIMARY KEY(depth,id));")?;
        self.connection.cached_execute(
            "DELETE FROM resolution_python_exports WHERE depth=?1",
            [depth],
        )?;
        let mut statement = self
            .connection
            .cached_prepare("SELECT id FROM resolution_modules WHERE depth=?1 ORDER BY id")?;
        let mut rows = statement.query([depth])?;
        while let Some(row) = rows.next()? {
            let file: i64 = row.get(0)?;
            let base:Option<(String,usize,usize)>=self.connection.cached_row("SELECT kind,start_byte,end_byte FROM bindings WHERE file_id=?1 AND kind IN ('python_all_reset','python_all_unknown','python_all_absent') ORDER BY start_byte DESC,id DESC LIMIT 1",[file],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let (kind, start, end) = base.unwrap_or_else(|| ("python_all_absent".into(), 0, 0));
            let after:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1
                FROM bindings b
                WHERE b.file_id=?1
                AND b.scope_id=0
                AND b.name='__all__'
                AND b.start_byte>=?2
                AND NOT EXISTS(SELECT 1
                FROM bindings s
                WHERE s.file_id=b.file_id
                AND s.kind IN ('python_all_reset','python_all_append','python_all_unknown','python_all_absent')
                AND b.start_byte>=s.start_byte
                AND b.start_byte<s.end_byte))",params![file,end],|r|r.get(0))?;
            let append:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND kind='python_all_append' AND start_byte>=?2)",params![file,start],|r|r.get(0))?;
            let unknown =
                kind == "python_all_unknown" || after || kind == "python_all_absent" && append;
            let allowed = if unknown {
                self.candidate_only = true;
                self.reason = Some("module has a dynamic or conditionally assigned __all__");
                true
            } else if kind == "python_all_absent" {
                !name.starts_with('_')
            } else {
                self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM bindings e JOIN bindings s ON s.file_id=e.file_id AND s.start_byte=CAST(e.module_name AS INTEGER)
                    WHERE e.file_id=?1 AND e.kind='python_all_entry' AND e.imported_name=?2 AND s.kind IN ('python_all_reset','python_all_append') AND s.start_byte>=?3)",params![file,name,start],|r|r.get(0))?
            };
            self.dependency("python_star_export", &format!("{file}:{name}"), allowed)?;
            if allowed {
                self.connection.cached_execute(
                    "INSERT INTO resolution_python_exports VALUES(?1,?2)",
                    params![depth, file],
                )?;
            }
        }
        drop(rows);
        drop(statement);
        self.connection.cached_execute("DELETE FROM resolution_modules WHERE depth=?1 AND id NOT IN (SELECT id FROM resolution_python_exports WHERE depth=?1)",[depth])?;
        Ok(())
    }

    pub(in crate::application::analysis::resolve) fn python_module_name(
        &mut self,
        name: &str,
        depth: usize,
    ) -> Result<()> {
        let (count, file): (usize, Option<i64>) = self.connection.cached_row(
            "SELECT COUNT(*),MIN(id) FROM resolution_modules WHERE depth=?1",
            [depth],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if count == 1 {
            if let Some(file) = file {
                self.python_file_name(file, name, depth)?;
            }
        } else if count > 1 {
            let snapshot = self.snapshot_modules(depth)?;
            self.connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS resolution_python_values(lookup_scope INTEGER,id INTEGER,PRIMARY KEY(lookup_scope,id));")?;
            let mut statement = self.connection.cached_prepare(
                "SELECT id FROM resolution_module_snapshots WHERE lookup_scope=?1 ORDER BY id",
            )?;
            let mut rows = statement.query([snapshot])?;
            while let Some(row) = rows.next()? {
                let file: i64 = row.get(0)?;
                self.connection
                    .cached_execute("DELETE FROM resolution_candidates", [])?;
                self.python_file_name(file, name, depth)?;
                self.connection.cached_execute("INSERT OR IGNORE INTO resolution_python_values SELECT ?1,id FROM resolution_candidates",[snapshot])?;
            }
            drop(rows);
            drop(statement);
            self.connection
                .cached_execute("DELETE FROM resolution_candidates", [])?;
            self.connection.cached_execute("INSERT INTO resolution_candidates SELECT id FROM resolution_python_values WHERE lookup_scope=?1",[snapshot])?;
            self.connection.cached_execute(
                "DELETE FROM resolution_python_values WHERE lookup_scope=?1",
                [snapshot],
            )?;
            self.release_modules(snapshot)?;
        }
        if self.rule != "language_builtin" {
            self.rule = "python_module_binding";
        }
        Ok(())
    }

    fn python_file_name(&mut self, file: i64, name: &str, depth: usize) -> Result<()> {
        let Some(target) = crate::application::analysis::resolve::context(
            self.connection,
            file,
            Some(Language::Python),
        )?
        else {
            return Ok(());
        };
        let position = usize::try_from(i64::MAX).unwrap_or(usize::MAX);
        let mut parts = name.split(['.', ':']).filter(|n| !n.is_empty());
        let Some(head) = parts.next() else {
            return Ok(());
        };
        let frame = self.external_scope()?;
        if self.binding(&target, 0, position, head)?.is_some() {
            self.expression(&target, 0, position, name, depth + 1)?;
        } else {
            self.scope_imports(&target, 0, head, depth + 1)?;
            for member in parts {
                self.members(member)?;
            }
            let found: bool = self.connection.cached_row(
                "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
                [],
                |r| r.get(0),
            )?;
            if !found
                && !self.has_externals()
                && self.binding(&target, 0, position, "__getattr__")?.is_some()
            {
                self.candidate_only = true;
                self.reason =
                    Some("module attribute may be supplied by a dynamic __getattr__ hook");
            }
        }
        self.finish_external_scope(frame, true)
    }
}
