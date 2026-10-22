// Purpose: Rust lookup의 타입·값·macro 공간과 경로 문맥 제한.
// Pattern: Simple Module.
// Usage: lexical·binding·module·member lookup이 같은 공간을 적용.
// Related: crates/kodstar-core/src/domain/resolve/namespaces.rs, crates/kodstar-core/src/application/analysis/resolve/lexical.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::resolve::namespaces::Namespace;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn rust_targets(&mut self) -> Result<()> {
        if self.language != Language::Rust || self.namespace == Namespace::Any {
            return Ok(());
        }
        self.prepared.rust(params![
            self.namespace.as_str(),
            if self.receiver {
                "references"
            } else {
                self.relation
            }
        ])?;
        Ok(())
    }
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn rust_exported_macro(
        &mut self,
        context: &crate::application::analysis::resolve::FileContext,
        scope: u32,
        module: &str,
        imported: Option<&str>,
    ) -> Result<bool> {
        if self.language != Language::Rust
            || self.namespace != Namespace::Macros
            || self.module_path(context, scope, module)? != "crate"
        {
            return Ok(false);
        }
        let Some(name) = imported.filter(|n| *n != "*") else {
            return Ok(false);
        };
        let key = context.language.lookup_key(name.trim_end_matches('!'));
        let count = self.connection.cached_execute(
            "INSERT
            OR IGNORE INTO resolution_candidates
            SELECT s.id
            FROM symbols s
            JOIN files f ON f.id=s.file_id
            JOIN bindings b ON b.file_id=s.file_id
            AND b.target_local=s.local_id
            WHERE s.language='rust'
            AND s.kind='macro'
            AND s.name_key=?1
            AND f.package=?2
            AND b.kind='rust_macro_export'",
            params![key, context.package],
        )?;
        self.dependency(
            "rust_macro_export",
            &format!("{}:{key}", context.package),
            count > 0,
        )?;
        if count == 0 {
            return Ok(false);
        }
        let conditional: bool = self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id WHERE b.kind='rust_macro_export' AND b.type_name IS NOT NULL)", [], |r| r.get(0))?;
        self.candidate_only |= conditional;
        if conditional {
            self.reason = Some("macro_export is conditional and configuration was not evaluated");
        }
        self.rule = "explicit_rust_macro_export";
        Ok(true)
    }

    pub(in crate::application::analysis::resolve) fn rust_implicit_exported_macro(
        &mut self,
        context: &crate::application::analysis::resolve::FileContext,
        scope: u32,
        name: &str,
    ) -> Result<bool> {
        if self.language != Language::Rust
            || self.namespace != Namespace::Macros
            || context.module != "crate"
            || crate::application::analysis::resolve::namespaces::nearest_module(
                self.connection,
                context.id,
                scope,
            )?
            .is_some()
        {
            return Ok(false);
        }
        self.rust_exported_macro(context, scope, "crate", Some(name))
    }
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn rust_macro_members(
        &mut self,
        member: &str,
    ) -> Result<bool> {
        if self.language != Language::Rust || self.namespace != Namespace::Macros {
            return Ok(false);
        }
        let saved = self.save_candidates()?;
        let mut statement = self.connection.cached_prepare("SELECT s.file_id,s.local_id,s.kind,s.qualified_name,f.module FROM resolution_saved r JOIN symbols s ON s.id=r.id JOIN files f ON f.id=s.file_id WHERE r.lookup_scope=?1 AND s.kind IN ('file','module') ORDER BY s.id")?;
        let records = statement.query_map([saved], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for record in records {
            let (file, scope, kind, name, module) = record?;
            let Some(context) = crate::application::analysis::resolve::context(
                self.connection,
                file,
                Some(Language::Rust),
            )?
            else {
                continue;
            };
            let module = if kind == "file" {
                module
            } else {
                format!("{module}::{name}")
            };
            self.module(
                &context,
                scope,
                &module,
                Some(member),
                self.member_depth + 1,
            )?;
        }
        self.connection.cached_execute(
            "DELETE FROM resolution_saved WHERE lookup_scope=?1",
            [saved],
        )?;
        Ok(true)
    }
}
