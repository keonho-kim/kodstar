// Purpose: native USR 색인으로 실제 C/C++ callee를 선택하고 미완료 의미 분석을 진단.
// Pattern: Resolution Adapter.
// Usage: resolver 준비와 호출 사실 dispatcher가 저장된 native 근거를 소비.
// Related: crates/kodstar-core/src/infrastructure/clang/projection.rs, crates/kodstar-core/src/application/analysis/resolve/inputs.rs.

use super::{FileContext, Lookup, inputs::Disposition};
use crate::domain::{
    diagnostics::{Error, Result},
    languages::Language,
};
use crate::infrastructure::index::cached::Sql;
use rusqlite::OptionalExtension;

pub(super) fn prepare(c: &Sql<'_>) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS resolution_clang_symbols(usr TEXT,defined INTEGER,target INTEGER,PRIMARY KEY(usr,defined,target)) WITHOUT ROWID;
        DELETE FROM resolution_clang_symbols;
        INSERT OR IGNORE INTO resolution_clang_symbols SELECT b.module_name,COALESCE(b.imported_name='definition',0),s.id
        FROM bindings b JOIN symbols s ON s.file_id=b.file_id AND s.local_id=b.target_local WHERE b.kind='clang_symbol';")?;
    Ok(())
}
impl Lookup<'_, '_> {
    pub(super) fn clang_call(&mut self, context: &FileContext) -> Result<Option<Disposition>> {
        if !self.prepared.native_calls
            || self.relation != "calls"
            || !matches!(context.language, Language::C | Language::Cpp)
        {
            return Ok(None);
        }
        let value:Option<(String,bool)>=self.connection.cached_row("SELECT b.module_name,f.condition IS NOT NULL
            FROM facts f JOIN bindings b ON b.file_id=f.file_id AND b.start_byte=f.start_byte AND b.end_byte=f.end_byte
            WHERE f.id=?1 AND b.kind='clang_call' LIMIT 1",[self.fact],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((data, conditional)) = value else {
            return Ok(None);
        };
        let target: crate::domain::resolve::native::Target<'_> = serde_json::from_str(&data)
            .map_err(|e| Error::caused_by("invalid native callee evidence", e))?;
        let Some(relative) = target.file.strip_prefix("/__kodstar_project/") else {
            return Ok(None);
        };
        let path = crate::infrastructure::cache::paths::path_bytes(std::path::Path::new(relative));
        self.connection.cached_execute("INSERT OR IGNORE INTO resolution_candidates
            SELECT n.target FROM resolution_clang_symbols n JOIN symbols s ON s.id=n.target JOIN files f ON f.id=s.file_id
            WHERE n.usr=?1 AND f.path=?2 AND s.start_byte<=?3 AND s.end_byte>?3",rusqlite::params![target.usr.as_ref(),path,target.position])?;
        let found: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |r| r.get(0),
        )?;
        if !found {
            self.connection.cached_execute(
                "INSERT OR IGNORE INTO resolution_candidates
                SELECT s.id FROM symbols s JOIN files f ON f.id=s.file_id
                WHERE f.path=?1 AND s.name=?2 AND s.start_byte<=?3 AND s.end_byte>?3
                AND s.kind IN ('function','method','constructor')",
                rusqlite::params![path, target.name.as_ref(), target.position],
            )?;
        }
        let conditional_target:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM cxx_conditions x JOIN resolution_candidates c ON c.id=x.id WHERE x.effective<>'')",[],|r|r.get(0))?;
        if conditional_target {
            self.connection
                .cached_execute("DELETE FROM resolution_candidates", [])?;
            return Ok(None);
        }
        if found {
            self.connection.cached_execute("WITH RECURSIVE providers(file) AS (
                SELECT id FROM files WHERE path=?2 UNION SELECT i.source FROM cxx_includes i JOIN providers p ON p.file=i.target)
                INSERT OR IGNORE INTO resolution_candidates SELECT n.target FROM resolution_clang_symbols n JOIN symbols s ON s.id=n.target
                WHERE n.usr=?1 AND s.file_id IN (SELECT file FROM providers)",rusqlite::params![target.usr.as_ref(),path])?;
        }
        self.cxx_targets()?;
        self.rule = "clang_semantic_call";
        self.candidate_only = conditional;
        if conditional {
            self.reason =
                Some("native callee is proven only in the selected preprocessing context");
        }
        let found: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |r| r.get(0),
        )?;
        self.dependency("clang_usr", &target.usr, found)?;
        if !found {
            self.external_target(&format!("clang:{}", target.usr))?;
            self.reason = Some("native callee has no indexed project definition");
        }
        Ok(Some(Disposition::Handled))
    }
}
