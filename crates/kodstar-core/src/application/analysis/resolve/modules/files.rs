// Purpose: Select indexed module files using concrete language path rules.
// Pattern: Simple Module.
// Usage: Module target interpretation asks for indexed files at a lookup depth.
// Related: crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs, crates/kodstar-core/src/application/analysis/resolve/indexed_paths.rs.

use crate::application::analysis::resolve::indexed_paths::{PathKind, normalize};
use crate::application::analysis::resolve::modules::Location;
use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::params;
use std::path::Path;

#[cfg(test)]
mod tests;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn module_files(
        &mut self,
        context: &FileContext,
        scope: u32,
        raw: &str,
        depth: usize,
    ) -> Result<Location> {
        // A depth is a lookup frame, never an accumulator shared by sibling requests.
        self.connection
            .cached_execute("DELETE FROM resolution_modules WHERE depth=?1", [depth])?;
        self.connection.cached_execute(
            "DELETE FROM resolution_module_owners WHERE depth=?1",
            [depth],
        )?;
        let parent = context.path.parent().unwrap_or_else(|| Path::new(""));
        if context.language == Language::Zig && raw == "root" {
            self.reason = Some("Zig root module depends on an unconfigured build entry");
            return Ok(Location::Project);
        }
        if let Some(include) = raw.strip_prefix("@erlang-include:") {
            self.connection.cached_execute(
                "DELETE FROM resolution_module_owners WHERE depth=?1",
                [depth],
            )?;
            if let Some(path) = normalize(parent, include) {
                self.indexed_path(&path, PathKind::ErlangInclude, depth)?;
            }
            return Ok(Location::Project);
        }
        if let Some(include) = raw.strip_prefix("@include:") {
            self.connection.cached_execute(
                "DELETE FROM resolution_module_owners WHERE depth=?1",
                [depth],
            )?;
            if let Some(path) = normalize(parent, include) {
                self.indexed_path(&path, PathKind::Include, depth)?;
            }
            return Ok(Location::Project);
        }
        let javascript = matches!(
            context.language,
            Language::JavaScript | Language::TypeScript
        );
        if javascript && !raw.starts_with('.') {
            self.connection.cached_execute(
                "DELETE FROM resolution_module_owners WHERE depth=?1",
                [depth],
            )?;
            if depth >= 32 {
                self.reason = Some("package import mapping cycle/depth limit");
                return Ok(Location::Project);
            }
            if raw.starts_with("node:") {
                return Ok(Location::External(raw.into()));
            }
            if self.javascript_paths(context, raw, depth)? {
                return Ok(Location::Project);
            }
            return self.javascript_packages(context, scope, raw, depth);
        }
        if context.language == Language::Go {
            return self.go_paths(context, raw, depth);
        }
        if context.language == Language::Swift {
            let location = self.swift_modules(context, raw, depth)?;
            if !matches!(location, Location::Unmapped) {
                return Ok(location);
            }
        }
        let module = self.module_path(context, scope, raw)?;
        self.namespace_files(context, &module, depth)?;
        if matches!(
            context.language,
            Language::Rust
                | Language::Python
                | Language::Erlang
                | Language::Kotlin
                | Language::Scala
                | Language::Java
                | Language::Swift
                | Language::CommonLisp
        ) {
            self.connection.cached_execute(
                "INSERT OR IGNORE INTO resolution_modules
                SELECT ?4,id FROM files
                WHERE language_id=(SELECT id FROM atoms WHERE value=?1)
                  AND package_id=(SELECT id FROM atoms WHERE value=?2)
                  AND module_id=(SELECT id FROM atoms WHERE value=?3) AND status_id=(SELECT id FROM atoms WHERE value='analyzed')",
                params![context.language.as_str(), context.package, module, depth],
            )?;
            if matches!(context.language, Language::Python | Language::Rust) {
                let found: bool = self.connection.cached_row(
                    "SELECT EXISTS(SELECT 1 FROM resolution_modules WHERE depth=?1)",
                    [depth],
                    |r| r.get(0),
                )?;
                self.dependency(
                    "module_provider",
                    &crate::domain::resolve::lookup_keys::module_provider(
                        context.language.as_str(),
                        &context.package,
                        &module,
                    ),
                    found,
                )?;
            }
        }
        if javascript
            || context.language == Language::Ruby && raw.starts_with('.')
            || context.language == Language::Zig && raw.ends_with(".zig")
        {
            if let Some(path) = normalize(parent, raw) {
                let kind = if javascript {
                    PathKind::JavaScript
                } else if context.language == Language::Ruby {
                    PathKind::Ruby
                } else {
                    PathKind::Zig
                };
                self.indexed_path(&path, kind, depth)?;
            }
            return Ok(Location::Project);
        }
        Ok(Location::Unmapped)
    }
}
