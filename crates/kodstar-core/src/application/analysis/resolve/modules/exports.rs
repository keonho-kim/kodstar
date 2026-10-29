// Purpose: 저장된 명시적 local·default·star·namespace 재수출 해석.
// Pattern: Simple Module.
// Usage: module lookup이 식별한 파일 집합과 요청 이름을 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs, crates/kodstar-core/src/infrastructure/parse/javascript/imports.rs.

use crate::application::analysis::resolve::modules::export_records::Export;
use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use rusqlite::params;

// The nearest declaration owns the export; function-local pub use never escapes its scope.
const EXPORT_SCOPE: &str = "EXISTS (
    WITH RECURSIVE parents(id,depth) AS (
      SELECT b.scope_id,0 UNION ALL SELECT s.parent,p.depth+1 FROM scopes s JOIN parents p ON s.local_id=p.id
      WHERE s.file_id=b.file_id AND s.parent IS NOT NULL AND p.depth<1024
    ), nearest(id,kind) AS (
      SELECT s.id,s.kind FROM symbols s JOIN parents p ON s.local_id=p.id
      WHERE s.file_id=b.file_id ORDER BY p.depth LIMIT 1
    ) SELECT 1 FROM nearest o WHERE
      (o.kind='file' AND NOT EXISTS(SELECT 1 FROM resolution_owner_snapshots n WHERE n.lookup_scope=m.lookup_scope AND n.file_id=b.file_id))
      OR EXISTS(SELECT 1 FROM resolution_owner_snapshots n WHERE n.lookup_scope=m.lookup_scope AND n.file_id=b.file_id AND n.owner_id=o.id)
)";

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn module_name(
        &mut self,
        context: &FileContext,
        name: &str,
        depth: usize,
    ) -> Result<()> {
        if context.language == crate::domain::languages::Language::Python {
            return self.python_module_name(name, depth);
        }
        let (head, tail) = name
            .split_once("::")
            .map_or((name, None), |(a, b)| (a, Some(b)));
        let previous = self.namespace;
        if self.language == crate::domain::languages::Language::Rust && tail.is_some() {
            self.namespace = crate::domain::resolve::namespaces::Namespace::Types;
        }
        let head = if matches!(
            self.language,
            crate::domain::languages::Language::Rust | crate::domain::languages::Language::Erlang
        ) {
            head.trim_end_matches('!')
        } else {
            head
        };
        let (head, arity) = if context.language == crate::domain::languages::Language::Elixir {
            head.rsplit_once('/')
                .and_then(|(n, a)| a.parse::<u32>().ok().map(|a| (n, Some(a))))
                .unwrap_or((head, None))
        } else {
            (head, None)
        };
        let name_filter = if context.language == crate::domain::languages::Language::Rust {
            "s.name_key_id IN (SELECT id FROM atoms WHERE value IN (?1,?4)) AND"
        } else {
            ""
        };
        self.connection.cached_execute(
            &include_str!("candidates.sql").replace("/* indexed name guard */", name_filter),
            params![
                head,
                depth,
                arity,
                context.language.lookup_key(head),
                context.id,
                self.namespace.as_str()
            ],
        )?;
        self.scala_targets()?;
        self.cxx_targets()?;
        let filtered = self.rust_targets();
        self.namespace = previous;
        filtered?;
        let found: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |r| r.get(0),
        )?;
        if found {
            if let Some(tail) = tail {
                self.members(tail)?;
            }
            return Ok(());
        }
        // Explicit names win over star exports. Star exports never forward `default`.
        let rust_key = (self.language == crate::domain::languages::Language::Rust)
            .then(|| context.language.lookup_key(head));
        if let Some(record) = self.single_export(head, rust_key.as_deref(), depth)? {
            self.resolve_export(context, &record, head, tail, depth)?;
            self.component_default(head, depth)?;
            self.rule = "explicit_module_export";
            return Ok(());
        }
        let snapshot = self.snapshot_modules(depth)?;
        let name_filter = if self.language == crate::domain::languages::Language::Rust {
            "b.name_key_id IN (SELECT id FROM atoms WHERE value IN (?1,?3)) AND"
        } else {
            ""
        };
        let named: bool = self.connection.cached_row(
            &format!("SELECT EXISTS(SELECT 1 FROM bindings b JOIN resolution_module_snapshots m ON m.id=b.file_id
             WHERE {name_filter} m.lookup_scope=?2 AND b.kind_id IN (SELECT id FROM atoms WHERE value IN ('export','export_type','commonjs_export_module','commonjs_export_exports')) AND (b.name=?1 OR b.name_key=?3) AND {EXPORT_SCOPE})"),
            params![head, snapshot,rust_key],
            |r| r.get(0),
        )?;
        let name_filter = if self.language == crate::domain::languages::Language::Rust {
            "b.name_key_id IN (SELECT id FROM atoms WHERE value IN (?1,?4,'*')) AND"
        } else {
            ""
        };
        let mut exports = self.connection.cached_prepare(&format!(
            "SELECT b.file_id,b.scope_id,b.module_name,b.imported_name,b.start_byte,b.kind,b.name,b.type_name
             FROM bindings b JOIN resolution_module_snapshots m ON m.id=b.file_id
             WHERE {name_filter} m.lookup_scope=?2 AND b.kind_id IN (SELECT id FROM atoms WHERE value IN ('export','export_type','commonjs_export_module','commonjs_export_exports'))
               AND (b.name=?1 OR b.name_key=?4 OR (?3=0 AND ?1<>'default' AND b.name='*')) AND {EXPORT_SCOPE} ORDER BY b.id"
        ))?;
        let records = exports.query_map(params![head, snapshot, named, rust_key], Export::read)?;
        for record in records {
            self.resolve_export(context, &record?, head, tail, depth)?;
        }
        self.rule = "explicit_module_export";
        self.release_modules(snapshot)?;
        self.component_default(head, depth)
    }

    fn component_default(&self, head: &str, depth: usize) -> Result<()> {
        if head == "default" && self.relation == "references" {
            self.connection.cached_execute(
                "INSERT
                OR IGNORE INTO resolution_candidates
                SELECT s.id
                FROM symbols s
                JOIN resolution_modules m ON m.id=s.file_id
                JOIN files f ON f.id=s.file_id
                WHERE m.depth=?1
                AND s.kind='file'
                AND f.language IN ('vue','svelte','astro')
                AND NOT EXISTS(SELECT 1
                FROM resolution_candidates)
                AND NOT EXISTS(SELECT 1
                FROM bindings b
                WHERE b.file_id=f.id
                AND b.kind='export_type'
                AND b.name='default')",
                [depth],
            )?;
        }
        Ok(())
    }

    fn resolve_export(
        &mut self,
        context: &FileContext,
        record: &Export,
        head: &str,
        tail: Option<&str>,
        depth: usize,
    ) -> Result<()> {
        if record.kind == "export_type"
            && !self.receiver
            && matches!(self.relation, "calls" | "references" | "indirect_call")
        {
            self.reason = Some("type-only export cannot establish a runtime value");
            return Ok(());
        }
        let Some(target_context) = crate::application::analysis::resolve::context(
            self.connection,
            record.file,
            Some(context.language),
        )?
        else {
            return Ok(());
        };
        if matches!(
            record.kind.as_str(),
            "commonjs_export_module" | "commonjs_export_exports"
        ) {
            let global = if record.kind == "commonjs_export_module" {
                "module"
            } else {
                "exports"
            };
            if self
                .binding(&target_context, record.scope, record.position, global)?
                .is_some()
            {
                return Ok(());
            }
        }
        if record.condition.as_deref() == Some(crate::domain::facts::CONDITIONAL_EXPORT) {
            self.candidate_only = true;
            self.reason = Some("conditional module export may not be defined");
        }
        if target_context.language == crate::domain::languages::Language::Rust
            && record.module.as_deref() == Some("")
        {
            if let Some(local) = record.imported.as_deref() {
                self.local_export(
                    &target_context,
                    record.scope,
                    record.position,
                    local,
                    tail,
                    depth + 1,
                )?;
            }
            return Ok(());
        }
        if let Some(module) = &record.module {
            let imported = if record.name == "*" {
                Some(head)
            } else {
                record.imported.as_deref()
            };
            let qualified = tail.map(|tail| {
                imported.map_or_else(|| tail.to_owned(), |name| format!("{name}::{tail}"))
            });
            self.module(
                &target_context,
                record.scope,
                module,
                qualified.as_deref().or(imported),
                depth + 1,
            )?;
        } else if let Some(local) = &record.imported {
            self.local_export(
                &target_context,
                record.scope,
                record.position,
                local,
                tail,
                depth + 1,
            )?;
        }
        Ok(())
    }

    fn local_export(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        local: &str,
        tail: Option<&str>,
        depth: usize,
    ) -> Result<()> {
        if context.language == crate::domain::languages::Language::Rust {
            let local = if self.namespace == crate::domain::resolve::namespaces::Namespace::Macros {
                format!("{}!", local.trim_end_matches('!'))
            } else {
                local.to_owned()
            };
            self.expression(context, scope, position, &local, depth)?;
            if let Some(tail) = tail {
                self.members(tail)?;
            }
            return Ok(());
        }
        let saved = self.save_candidates()?;
        self.connection.cached_execute(
            "INSERT OR IGNORE INTO resolution_candidates SELECT s.id FROM symbols s JOIN symbols o ON o.id=s.owner_id
             WHERE s.file_id=?1 AND s.name=?2 AND o.kind IN ('file','module')", params![context.id,local])?;
        self.rust_targets()?;
        let found: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |row| row.get(0),
        )?;
        // A local export names the lexical binding, which may itself be an import/alias.
        // Export records are not lexical declarations and are excluded by binding().
        if !found {
            self.expression(context, scope, position, local, depth)?;
        }
        if let Some(tail) = tail {
            self.members(tail)?;
        }
        self.restore_candidates(saved)?;
        Ok(())
    }
}
