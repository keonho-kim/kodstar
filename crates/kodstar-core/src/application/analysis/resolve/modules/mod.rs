// Purpose: 명시적 import·module·재수출 대상 해석.
// Pattern: Simple Module.
// Usage: Lookup의 lexical·import 해석이 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/application/analysis/resolve/lexical.rs.

pub(super) mod export_records;
pub(super) mod exports;
mod files;
pub(super) mod paths;
pub(super) mod scopes;

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;

pub(in crate::application::analysis::resolve) enum Location {
    Unmapped,
    Project,
    External(String),
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn module(
        &mut self,
        context: &FileContext,
        scope: u32,
        module: &str,
        imported: Option<&str>,
        depth: usize,
    ) -> Result<()> {
        self.module_mode(context, scope, module, imported, depth, false)
    }

    pub(in crate::application::analysis::resolve) fn module_mode(
        &mut self,
        context: &FileContext,
        scope: u32,
        module: &str,
        imported: Option<&str>,
        depth: usize,
        python_star: bool,
    ) -> Result<()> {
        let frame = self.external_scope()?;
        self.module_inner(context, scope, module, imported, depth, python_star)?;
        self.finish_external_scope(frame, true)
    }

    fn module_inner(
        &mut self,
        context: &FileContext,
        scope: u32,
        module: &str,
        imported: Option<&str>,
        depth: usize,
        python_star: bool,
    ) -> Result<()> {
        if depth >= 32 {
            self.reason = Some("alias or reexport cycle/depth limit");
            return Ok(());
        }
        let saved = self.save_candidates()?;
        self.module_targets(context, scope, module, imported, depth, python_star)?;
        self.restore_candidates(saved)?;
        Ok(())
    }

    fn module_targets(
        &mut self,
        context: &FileContext,
        scope: u32,
        module: &str,
        imported: Option<&str>,
        depth: usize,
        python_star: bool,
    ) -> Result<()> {
        self.connection
            .cached_execute("DELETE FROM resolution_modules WHERE depth=?1", [depth])?;
        if matches!(context.language, Language::C | Language::Cpp)
            && module.starts_with("@include:")
        {
            self.cxx_include_condition(context, scope, module)?;
        }
        let location = self.module_files(context, scope, module, depth)?;
        if self.rust_exported_macro(context, scope, module, imported)? {
            return Ok(());
        }
        let found: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_modules WHERE depth=?1)",
            [depth],
            |row| row.get(0),
        )?;
        if context.language != Language::Go {
            self.dependency("module", &format!("{}:{module}", context.package), found)?;
        }
        if !found {
            if python_star {
                self.candidate_only = true;
                self.reason = Some("external wildcard module export list is not indexed");
            }
            if context.language == Language::Rust && !matches!(location, Location::Project) {
                let normalized = self.module_path(context, scope, module)?;
                if let Some((parent, owner)) = normalized.rsplit_once("::") {
                    let path = imported
                        .filter(|n| *n != "*")
                        .map_or_else(|| owner.to_owned(), |n| format!("{owner}::{n}"));
                    let previous = self.namespace;
                    if imported.is_none() || imported == Some("*") {
                        self.namespace = crate::domain::resolve::namespaces::Namespace::Types;
                    }
                    let result = self.module(context, scope, parent, Some(&path), depth + 1);
                    self.namespace = previous;
                    result?;
                    let identified: bool = self.connection.cached_row(
                        "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
                        [],
                        |r| r.get(0),
                    )?;
                    if identified {
                        return Ok(());
                    }
                }
            }
            if matches!(&location, Location::Project) {
                self.reason
                    .get_or_insert("configured or explicit local module target is not indexed");
                return Ok(());
            }

            let redirect = if let Location::External(target) = location {
                Some(target)
            } else {
                None
            };
            let module = redirect.as_deref().unwrap_or(module);
            if matches!(
                context.language,
                Language::Java
                    | Language::Kotlin
                    | Language::Scala
                    | Language::CSharp
                    | Language::Php
            ) && let Some((parent, owner)) =
                module.rsplit_once(if context.language == Language::Php {
                    '\\'
                } else {
                    '.'
                })
            {
                let qualified = imported
                    .filter(|n| *n != "*")
                    .map_or_else(|| owner.to_owned(), |name| format!("{owner}::{name}"));
                return self.module(context, scope, parent, Some(&qualified), depth + 1);
            }
            if !module.starts_with('.')
                && !matches!(
                    module.split("::").next(),
                    Some("crate" | "self" | "super" | "")
                )
            {
                self.external_target(&format!(
                    "{}:module:{module}{}",
                    context.language.as_str(),
                    imported.map(|name| format!("::{name}")).unwrap_or_default()
                ))?;
                self.reason = Some("external module API is not indexed");
                self.rule = "explicit_external_import";
            } else {
                self.reason = Some("explicit project module was not found");
            }
            return Ok(());
        }
        if python_star {
            self.python_star_files(imported.unwrap_or(""), depth)?;
        }
        if imported.is_none() || imported == Some("*") {
            self.connection.cached_execute(
                "INSERT
                OR IGNORE INTO resolution_candidates
                SELECT s.id
                FROM symbols s
                JOIN resolution_modules m ON m.id=s.file_id
                WHERE m.depth=?1
                AND (s.id IN (SELECT owner_id
                FROM resolution_module_owners
                WHERE depth=?1)
                OR s.kind='file'
                AND NOT EXISTS(SELECT 1
                FROM resolution_module_owners n
                WHERE n.depth=m.depth
                AND n.file_id=s.file_id))",
                [depth],
            )?;
            return Ok(());
        }
        if matches!(context.language, Language::C | Language::Cpp)
            && module.starts_with("@include:")
        {
            self.cxx_included_modules(depth)?;
        }
        self.module_name(context, imported.unwrap_or(""), depth)
    }
}
