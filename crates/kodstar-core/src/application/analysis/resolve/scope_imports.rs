// Purpose: scope에서 명시한 wildcard import의 이름 해석.
// Pattern: Simple Module.
// Usage: lexical lookup이 직접 binding을 찾지 못한 경우 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(super) fn scope_imports(
        &mut self,
        context: &FileContext,
        mut scope: u32,
        name: &str,
        depth: usize,
    ) -> Result<bool> {
        for _ in 0..1025 {
            if context.language == Language::Go {
                let mut defaults=self.connection.cached_prepare("SELECT module_name FROM bindings WHERE file_id=?1 AND scope_id=?2 AND kind='go_import' ORDER BY id")?;
                let defaults =
                    defaults.query_map(params![context.id, scope], |r| r.get::<_, String>(0))?;
                let mut present = false;
                for module in defaults {
                    let module = module?;
                    self.connection
                        .cached_execute("DELETE FROM resolution_modules WHERE depth=64", [])?;
                    self.module_files(context, scope, &module, 64)?;
                    let matches:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM files f JOIN resolution_modules m ON m.id=f.id WHERE m.depth=64 AND f.declared_module=?1)",[name],|r|r.get(0))?;
                    if matches {
                        self.module(context, scope, &module, None, depth + 1)?;
                        present = true;
                    }
                }
                if present {
                    return Ok(true);
                }
            }
            let mut imports = self.connection.cached_prepare("SELECT module_name,imported_name FROM bindings WHERE file_id=?1 AND scope_id=?2 AND name='*' AND kind IN ('import','export','include') AND module_name IS NOT NULL ORDER BY id")?;
            let records = imports.query_map(params![context.id, scope], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            let mut present = false;
            for module in records {
                let (module, prefix) = module?;
                if context.language == Language::Elixir {
                    let excluded:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND scope_id=?2 AND kind='elixir_import_exclusion' AND module_name=?3 AND name=?4)",params![context.id,scope,module,name],|r|r.get(0))?;
                    if excluded {
                        continue;
                    }
                }
                present = true;
                let imported = prefix
                    .filter(|p| p != "*")
                    .map_or_else(|| name.to_owned(), |prefix| format!("{prefix}::{name}"));
                self.module_mode(
                    context,
                    scope,
                    &module,
                    Some(&imported),
                    depth + 1,
                    context.language == Language::Python,
                )?;
            }
            if present
                && (context.language != Language::Python
                    || self.has_externals()
                    || self.candidate_only
                    || self.connection.cached_row(
                        "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
                        [],
                        |r| r.get::<_, bool>(0),
                    )?)
            {
                return Ok(true);
            }
            let parent: Option<u32> = self.connection.cached_row(
                "SELECT parent FROM scopes WHERE file_id=?1 AND local_id=?2",
                params![context.id, scope],
                |r| r.get(0),
            )?;
            let Some(parent) = parent else { break };
            scope = parent;
        }
        Ok(false)
    }

    pub(super) fn go_import_matches(
        &mut self,
        context: &FileContext,
        module: &str,
        name: &str,
    ) -> Result<bool> {
        self.connection
            .cached_execute("DELETE FROM resolution_modules WHERE depth=64", [])?;
        self.module_files(context, 0, module, 64)?;
        Ok(self.connection.cached_row("SELECT NOT EXISTS(SELECT 1 FROM resolution_modules WHERE depth=64) OR EXISTS(SELECT 1 FROM files f JOIN resolution_modules m ON m.id=f.id WHERE m.depth=64 AND f.declared_module=?1)",[name],|r|r.get(0))?)
    }
}
