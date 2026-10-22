// Purpose: Lisp reader package 기준 전역·명시적 package 이름 조회.
// Pattern: Simple Module.
// Usage: lexical 해석이 지역 binding 이후 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/infrastructure/parse/lisp/packages.rs.

pub(super) mod methods;

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use rusqlite::params;

impl Lookup<'_, '_> {
    fn lisp_reader_package(&self, context: &FileContext, scope: u32) -> Result<String> {
        Ok(self.connection.cached_row(
            "WITH RECURSIVE parents(id,depth) AS (VALUES(?2,0)
            UNION ALL
            SELECT s.parent,p.depth+1
            FROM parents p
            JOIN scopes s ON s.file_id=?1
            AND s.local_id=p.id
            WHERE s.parent IS NOT NULL
            AND p.depth<1024)
            SELECT b.module_name
            FROM parents p
            JOIN bindings b ON b.file_id=?1
            AND b.scope_id=p.id
            AND b.kind='lisp_scope_package'
            ORDER BY p.depth LIMIT 1",
            params![context.id, scope],
            |r| r.get(0),
        )?)
    }

    pub(in crate::application::analysis::resolve) fn lisp_global(
        &mut self,
        context: &FileContext,
        scope: u32,
        key: &str,
    ) -> Result<()> {
        let package = self.lisp_reader_package(context, scope)?;
        self.lisp_package_candidates(context, &package, key, false)?;
        if !self.lisp_found()?
            && crate::application::analysis::resolve::builtins::builtin(
                context.language,
                key,
                self.relation,
            )
        {
            self.external_target(&format!("builtin:commonlisp:{key}"))?;
            self.rule = "language_builtin";
        }
        Ok(())
    }

    pub(in crate::application::analysis::resolve) fn lisp_qualified(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        package: &str,
        symbol: &str,
        external_only: bool,
    ) -> Result<()> {
        if !external_only && package == self.lisp_reader_package(context, scope)? {
            return self.bare(context, scope, position, symbol, 0);
        }
        self.lisp_package_candidates(
            context,
            package,
            &context.language.lookup_key(symbol),
            external_only,
        )
    }

    fn lisp_package_candidates(
        &mut self,
        context: &FileContext,
        package: &str,
        key: &str,
        external_only: bool,
    ) -> Result<()> {
        self.connection.cached_execute(
            include_str!("accessible.sql"),
            params![
                context.package,
                package,
                key,
                external_only,
                self.namespace.as_str()
            ],
        )?;
        let found = self.lisp_found()?;
        self.dependency(
            "lisp_package_name",
            &format!("{package}:{key}:{}", self.namespace.as_str()),
            found,
        )?;
        self.rule = "lisp_package_binding";
        if !found {
            self.reason = Some("no matching definition in the statically declared Lisp package");
        }
        Ok(())
    }

    fn lisp_found(&self) -> Result<bool> {
        Ok(self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |r| r.get(0),
        )?)
    }

    pub(in crate::application::analysis::resolve) fn lisp_package_reference(
        &mut self,
        context: &FileContext,
        package: &str,
    ) -> Result<()> {
        self.connection.cached_execute("INSERT OR IGNORE INTO resolution_candidates SELECT s.id FROM bindings b JOIN symbols s ON s.file_id=b.file_id AND s.local_id=b.target_local JOIN files f ON f.id=b.file_id WHERE b.kind='lisp_pkg_def' AND b.module_name=?1 AND f.package=?2",params![package,context.package])?;
        let found = self.lisp_found()?;
        self.dependency("lisp_package", package, found)?;
        self.rule = "lisp_package_declaration";
        if !found {
            self.external_target(&format!("lisp-package:{package}"))?;
        }
        Ok(())
    }
}
