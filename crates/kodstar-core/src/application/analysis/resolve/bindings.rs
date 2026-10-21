// Purpose: Scope binding and lexical shadowing selection.
// Pattern: Simple Module.
// Usage: lexical expressions select their directly visible binding.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/application/analysis/resolve/python/mod.rs.

use crate::application::analysis::resolve::{Binding, FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::{OptionalExtension, params};

impl Lookup<'_, '_> {
    pub(super) fn binding(
        &mut self,
        context: &FileContext,
        mut scope: u32,
        position: usize,
        name: &str,
    ) -> Result<Option<Binding>> {
        let key = context.language.lookup_key(name);
        let pascal_class: Option<u32> = if context.language == Language::Pascal {
            self.connection.query_row("SELECT CASE WHEN o.file_id=?1 AND o.kind IN ('class','interface','type') THEN o.local_id END FROM facts f JOIN symbols s ON s.id=f.source_id LEFT JOIN symbols o ON o.id=s.owner_id WHERE f.id=?2",params![context.id,self.fact],|r|r.get(0)).optional()?.flatten()
        } else {
            None
        };
        let mut class_visited = false;
        let ruby_method=context.language==Language::Ruby && self.connection.query_row("SELECT EXISTS(SELECT 1 FROM facts f JOIN symbols s ON s.id=f.source_id WHERE f.id=?1 AND s.kind IN ('method','function'))",[self.fact],|r|r.get::<_,bool>(0))?;
        let module = if context.language == Language::Elixir {
            crate::application::analysis::resolve::namespaces::nearest_module(
                self.connection,
                context.id,
                scope,
            )?
            .map(|m| m.0)
        } else {
            None
        };
        for _ in 0..1025 {
            if Some(scope) == pascal_class {
                class_visited = true;
            }
            if scope == 0
                && !class_visited
                && let Some(class) = pascal_class
            {
                scope = class;
                class_visited = true;
            }
            if let Some(target) = self.python_scope(context, scope, &key)? {
                let Some(target) = target else {
                    return Ok(None);
                };
                scope = target;
            }
            let binding = self.prepared.binding(params![
                context.id,
                scope,
                key,
                position,
                context.language.as_str(),
                self.namespace.as_str(),
                key.trim_end_matches('!'),
                ruby_method
            ])?;
            self.dependency(
                &format!("scope:{}", self.namespace.as_str()),
                &format!("{}:{scope}:{key}", context.id),
                binding.is_some(),
            )?;
            if let Some(binding) = binding {
                if context.language == Language::Elixir
                    && binding.kind == "function"
                    && module != Some(binding.scope)
                {
                    return Ok(None);
                }
                if context.language == Language::Python
                    && binding.position > position
                    && !matches!(binding.kind.as_str(), "function" | "type" | "parameter")
                {
                    return Ok(Some(Binding {
                        scope: binding.scope,
                        target: None,
                        module: None,
                        imported: None,
                        type_name: None,
                        kind: "local".into(),
                        position: binding.position,
                    }));
                }
                if binding.kind == "go_import"
                    && let Some(module) = binding.module.as_deref()
                    && !self.go_import_matches(context, module, name)?
                {
                    return Ok(None);
                }
                if binding.kind == "commonjs_import"
                    && name != "require"
                    && self
                        .binding(context, binding.scope, position, "require")?
                        .is_some()
                {
                    return Ok(Some(Binding {
                        scope: binding.scope,
                        target: None,
                        module: None,
                        imported: None,
                        type_name: None,
                        kind: "local".into(),
                        position: binding.position,
                    }));
                }
                if binding.position > position
                    && !matches!(
                        binding.kind.as_str(),
                        "function"
                            | "lisp_local_function"
                            | "type"
                            | "import"
                            | "export"
                            | "rust_constructor"
                            | "value_constructor"
                            | "value"
                            | "razor_injection"
                    )
                    && !matches!(
                        context.language,
                        Language::Python | Language::JavaScript | Language::TypeScript
                    )
                {
                    // Rust/C block bindings begin at their declaration; continue in the lexical parent.
                } else {
                    return Ok(Some(binding));
                }
            }
            let parent: Option<u32> = self.connection.query_row(
                "SELECT parent FROM scopes WHERE file_id=?1 AND local_id=?2",
                params![context.id, scope],
                |row| row.get(0),
            )?;
            match parent {
                Some(parent) => scope = parent,
                None => break,
            }
        }
        Ok(None)
    }
}
