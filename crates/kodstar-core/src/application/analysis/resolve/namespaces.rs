// Purpose: module 파일 후보를 요청한 namespace 소유 범위로 제한.
// Pattern: Simple Module.
// Usage: module_files가 정규화한 module 이름과 depth를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs, crates/kodstar-core/src/application/analysis/resolve/modules/exports.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::{OptionalExtension, params};

pub(super) fn nearest_module(
    c: &Connection,
    file: i64,
    scope: u32,
) -> Result<Option<(u32, String)>> {
    Ok(c.cached_row("WITH RECURSIVE parents(id,depth) AS (
        SELECT ?2,0 UNION ALL SELECT s.parent,p.depth+1 FROM scopes s JOIN parents p ON s.local_id=p.id
        WHERE s.file_id=?1 AND s.parent IS NOT NULL AND p.depth<1024
      ) SELECT n.local_id,n.qualified_name FROM symbols n JOIN parents p ON n.local_id=p.id
        WHERE n.file_id=?1 AND n.kind='module' ORDER BY p.depth LIMIT 1",params![file,scope],|r|Ok((r.get(0)?,r.get(1)?))).optional()?)
}

impl Lookup<'_, '_> {
    pub(super) fn package_peers(&self, context: &FileContext, name: &str) -> Result<bool> {
        if matches!(
            context.language,
            Language::Go | Language::Kotlin | Language::Scala
        ) && context.declared_namespace.is_empty()
        {
            return Ok(false);
        }
        let count = self.connection.cached_execute(
            "INSERT OR IGNORE INTO resolution_candidates
             SELECT s.id FROM symbols s JOIN files f ON f.id=s.file_id
             JOIN symbols o ON o.id=s.owner_id
             WHERE f.language_id=(SELECT id FROM atoms WHERE value=?1) AND f.module_id=(SELECT id FROM atoms WHERE value=?2) AND f.package_id=(SELECT id FROM atoms WHERE value=?3) AND s.name_key_id=(SELECT id FROM atoms WHERE value=?4)
               AND f.declared_module_id=(SELECT id FROM atoms WHERE value=?5) AND o.kind_id IN (SELECT id FROM atoms WHERE value IN ('file','module'))",
            params![
                context.language.as_str(),
                context.module,
                context.package,
                name,
                context.declared_namespace
            ],
        )?;
        self.scala_targets()?;
        if self.language == Language::Scala {
            return Ok(self.connection.cached_row(
                "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
                [],
                |r| r.get(0),
            )?);
        }
        Ok(count > 0)
    }

    pub(super) fn namespace_files(
        &self,
        context: &FileContext,
        module: &str,
        depth: usize,
    ) -> Result<()> {
        self.connection.cached_execute(
            "DELETE FROM resolution_module_owners WHERE depth=?1",
            [depth],
        )?;
        if !matches!(
            context.language,
            Language::Rust | Language::Cpp | Language::CSharp | Language::Php | Language::Elixir
        ) {
            return Ok(());
        }
        if context.language == Language::Cpp
            && (module.contains('.') || module.contains('/') || module.contains('@'))
        {
            return Ok(());
        }
        let normalized = module.replace(['.', '\\'], "::");
        self.connection.cached_execute(
            "INSERT OR IGNORE INTO resolution_module_owners
            SELECT ?4,s.file_id,s.id FROM symbols s JOIN files f ON f.id=s.file_id
            WHERE s.kind='module' AND s.language=?1 AND f.package=?2 AND f.status='analyzed'
              AND (CASE WHEN s.language='rust' THEN f.module||'::'||replace(s.qualified_name,'r#','')
                        ELSE replace(replace(s.qualified_name,'.','::'),char(92),'::') END)=?3",
            params![
                context.language.as_str(),
                context.package,
                normalized,
                depth
            ],
        )?;
        self.connection.cached_execute("INSERT OR IGNORE INTO resolution_modules SELECT depth,file_id FROM resolution_module_owners WHERE depth=?1",[depth])?;
        Ok(())
    }

    pub(super) fn namespace_peers(
        &self,
        context: &FileContext,
        scope: u32,
        name: &str,
    ) -> Result<bool> {
        if !matches!(context.language, Language::CSharp | Language::Php) {
            return Ok(false);
        }
        let count=self.connection.cached_execute("WITH RECURSIVE parents(id,depth) AS (
            SELECT ?2,0 UNION ALL SELECT s.parent,p.depth+1 FROM scopes s JOIN parents p ON s.local_id=p.id
            WHERE s.file_id=?1 AND s.parent IS NOT NULL AND p.depth<1024
          ), nearest(name) AS (
            SELECT replace(replace(n.qualified_name,'.','::'),char(92),'::') FROM symbols n JOIN parents p ON n.local_id=p.id
            WHERE n.file_id=?1 AND n.kind='module' ORDER BY p.depth LIMIT 1
          ) INSERT OR IGNORE INTO resolution_candidates
            SELECT s.id FROM symbols s JOIN symbols n ON n.id=s.owner_id JOIN files f ON f.id=s.file_id
            WHERE s.language=?3 AND f.package=?4 AND s.name_key=?5
              AND (n.kind='module' AND replace(replace(n.qualified_name,'.','::'),char(92),'::') IN (SELECT name FROM nearest)
                   OR n.kind='file' AND NOT EXISTS(SELECT 1 FROM nearest))",
            params![context.id,scope,context.language.as_str(),context.package,name])?;
        Ok(count > 0)
    }

    pub(super) fn declared_namespace(&self, context: &FileContext, name: &str) -> Result<bool> {
        if context.language == Language::Swift {
            return Ok(self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND kind='import' AND module_name=?2 AND name='*')",params![context.id,name],|r|r.get(0))?);
        }
        if matches!(context.language, Language::Kotlin | Language::Java) {
            return Ok(self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM files WHERE language=?3 AND package=?1 AND module=?2 AND status='analyzed')",params![context.package,name.replace("::","."),context.language.as_str()],|r|r.get(0))?);
        }
        if !matches!(
            context.language,
            Language::CSharp | Language::Php | Language::Cpp | Language::Elixir
        ) {
            return Ok(false);
        }
        let name = name.replace(['.', '\\'], "::");
        Ok(self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM symbols s JOIN files f ON f.id=s.file_id
            WHERE s.language=?1 AND f.package=?2 AND s.kind='module'
              AND replace(replace(s.qualified_name,'.','::'),char(92),'::')=?3)",
            params![context.language.as_str(), context.package, name],
            |r| r.get(0),
        )?)
    }
}
