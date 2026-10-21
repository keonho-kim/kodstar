// Purpose: PHP class/function/constant 공간과 namespace·별칭·전역 fallback 해석.
// Pattern: Simple Module.
// Usage: lexical 해석과 명시적 PHP import marker가 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/infrastructure/parse/php/imports.rs.

pub(super) mod members;
pub(super) mod traits;

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::resolve::namespaces::Namespace;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn php_expression(
        &mut self,
        c: &FileContext,
        scope: u32,
        position: usize,
        raw: &str,
        depth: usize,
    ) -> Result<bool> {
        if let Some(name) = raw.strip_prefix("@php-new:") {
            let previous = self.namespace;
            self.namespace = Namespace::Types;
            let result = self.expression(c, scope, position, name, depth + 1);
            self.namespace = previous;
            result?;
            return Ok(true);
        }
        if raw.contains(['.', '(', ')', '<', '>', '{', '}']) {
            return Ok(false);
        }
        if raw.contains("::") {
            if raw.starts_with('$') {
                return Ok(false);
            }
            if self.namespace == Namespace::Types {
                self.php_absolute(c, raw)?;
            } else if let Some((receiver, member)) = raw.rsplit_once("::") {
                self.type_expression(c, scope, position, receiver, depth + 1)?;
                self.members(member)?;
            }
            return Ok(true);
        }
        if raw.starts_with('$') {
            self.reason = Some("dynamic PHP variable call has no established callable target");
            return Ok(true);
        }
        if raw.is_empty() {
            return Ok(false);
        }
        if self.namespace == Namespace::Types
            && matches!(
                raw.to_ascii_lowercase().as_str(),
                "self" | "static" | "parent"
            )
        {
            self.connection.execute("INSERT
                OR IGNORE INTO resolution_candidates WITH RECURSIVE parents(id,depth) AS (VALUES(?2,0)
                UNION ALL
                SELECT s.parent,p.depth+1
                FROM parents p
                JOIN scopes s ON s.file_id=?1
                AND s.local_id=p.id
                WHERE s.parent IS NOT NULL
                AND p.depth<1024)
                SELECT s.id
                FROM parents p
                JOIN symbols s ON s.file_id=?1
                AND s.local_id=p.id
                WHERE s.kind IN ('class','trait','enum')
                ORDER BY p.depth LIMIT 1",params![c.id,scope])?;
            if raw.eq_ignore_ascii_case("parent") {
                self.connection.execute_batch(
                    "DELETE
                    FROM resolution_members; INSERT
                    OR IGNORE INTO resolution_members
                    SELECT r.target_id
                    FROM resolution_candidates c
                    JOIN relationships r ON r.source_id=c.id
                    JOIN facts f ON f.id=r.fact_id
                    WHERE f.relation='inherits'
                    AND r.resolution='resolved'; DELETE
                    FROM resolution_candidates; INSERT INTO resolution_candidates
                    SELECT id
                    FROM resolution_members;",
                )?;
            } else if raw.eq_ignore_ascii_case("static") {
                self.candidate_only = true;
                self.reason = Some("PHP late static binding runtime receiver is not established");
            }
            return Ok(true);
        }
        let current = crate::application::analysis::resolve::namespaces::nearest_module(
            self.connection,
            c.id,
            scope,
        )?
        .map_or_else(String::new, |m| m.1);
        if let Some(name) = raw.strip_prefix('\\') {
            self.php_absolute(c, name)?;
            return Ok(true);
        }
        if let Some(name) = raw.strip_prefix("namespace\\") {
            self.php_absolute(c, &format!("{current}::{name}"))?;
            return Ok(true);
        }
        let (first, tail) = raw
            .split_once('\\')
            .map_or((raw, None), |(a, b)| (a, Some(b)));
        let previous = self.namespace;
        if tail.is_some() {
            self.namespace = Namespace::Types;
        }
        let binding = self.binding(c, scope, position, first);
        self.namespace = previous;
        if let Some(binding) = binding? {
            if let Some(module) = binding.module {
                let path = tail.map_or_else(|| module.clone(), |t| format!("{module}\\{t}"));
                self.php_absolute(c, &path)?;
                if !self.php_found()? {
                    self.external_target(&format!("php:import:{path}"))?;
                    self.rule = "explicit_external_import";
                }
                return Ok(true);
            }
            if binding.target.is_some() {
                self.connection.execute(
                    "INSERT
                    OR IGNORE INTO resolution_candidates
                    SELECT s.id
                    FROM bindings b
                    JOIN symbols s ON s.file_id=b.file_id
                    AND s.local_id=b.target_local
                    WHERE b.file_id=?1
                    AND b.scope_id=?2
                    AND b.namespace=?3
                    AND (?3 IN ('type','function')
                    AND b.name_key=?4 COLLATE NOCASE
                    OR ?3 NOT IN ('type','function')
                    AND b.name_key=?4)",
                    params![c.id, binding.scope, self.namespace.as_str(), raw],
                )?;
                self.rule = "lexical_binding";
                return Ok(true);
            }
            self.reason = Some("PHP lexical binding has no established target");
            return Ok(true);
        }
        self.php_absolute(
            c,
            &if current.is_empty() {
                raw.into()
            } else {
                format!("{current}::{raw}")
            },
        )?;
        if !self.php_found()?
            && tail.is_none()
            && !current.is_empty()
            && matches!(self.namespace, Namespace::Functions | Namespace::Values)
        {
            self.php_absolute(c, raw)?;
            self.rule = "php_global_fallback";
        }
        Ok(true)
    }

    pub(in crate::application::analysis::resolve) fn php_absolute(
        &mut self,
        c: &FileContext,
        path: &str,
    ) -> Result<()> {
        let path = path
            .trim_start_matches('\\')
            .replace('\\', "::")
            .trim_start_matches("::")
            .to_owned();
        self.connection.execute(
            "INSERT
            OR IGNORE INTO resolution_candidates
            SELECT s.id
            FROM symbols s
            JOIN files f ON f.id=s.file_id
            JOIN symbols o ON o.id=s.owner_id
            WHERE f.language='php'
            AND f.package=?1
            AND s.qualified_name=?2 COLLATE NOCASE
            AND o.kind IN ('file','module')
            AND (?3='any'
            OR ?3='type'
            AND s.kind IN ('class','interface','trait','enum','type','module')
            OR ?3='function'
            AND s.kind='function'
            OR ?3='value'
            AND s.kind='field'
            AND s.name=?4)",
            params![
                c.package,
                path,
                self.namespace.as_str(),
                path.rsplit("::").next().unwrap_or(&path)
            ],
        )?;
        self.rule = "php_namespace_binding";
        self.dependency(
            "php_name",
            &format!("{path}:{}", self.namespace.as_str()),
            self.php_found()?,
        )?;
        Ok(())
    }

    fn php_found(&self) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |r| r.get(0),
        )?)
    }

    pub(in crate::application::analysis::resolve) fn php_import_reference(
        &mut self,
        c: &FileContext,
        expression: &str,
    ) -> Result<bool> {
        let (namespace, path) = if let Some(p) = expression.strip_prefix("@php-import-type:") {
            (Namespace::Types, p)
        } else if let Some(p) = expression.strip_prefix("@php-import-function:") {
            (Namespace::Functions, p)
        } else if let Some(p) = expression.strip_prefix("@php-import-value:") {
            (Namespace::Values, p)
        } else {
            return Ok(false);
        };
        let previous = self.namespace;
        self.namespace = namespace;
        let result = self.php_absolute(c, path);
        self.namespace = previous;
        result?;
        if !self.php_found()? {
            self.external_target(&format!("php:import:{path}"))?;
            self.rule = "explicit_external_import";
        }
        Ok(true)
    }
}
