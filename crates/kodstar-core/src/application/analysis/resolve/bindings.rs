// Purpose: Scope binding and lexical shadowing selection.
// Pattern: Simple Module.
// Usage: lexical expressions select their directly visible binding.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/application/analysis/resolve/python/mod.rs.

use crate::application::analysis::resolve::{Binding, FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(super) fn binding(
        &mut self,
        context: &FileContext,
        mut scope: u32,
        position: usize,
        name: &str,
    ) -> Result<Option<Binding>> {
        let key = context.language.lookup_key(name);
        let ruby_method=context.language==Language::Ruby && self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM symbols s WHERE s.id=?1 AND s.kind IN ('method','function'))",[self.source],|r|r.get::<_,bool>(0))?;
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
            if let Some(target) = self.python_scope(context, scope, &key)? {
                let Some(target) = target else {
                    return Ok(None);
                };
                scope = target;
            }
            let binding = self.prepared.reuse.borrow_mut().binding(
                self.connection.connection(),
                context.id,
                super::reuse::Key {
                    scope,
                    position,
                    language: context.language.as_str(),
                    namespace: self.namespace.as_str(),
                    ruby_method,
                    name: &key,
                },
                || {
                    self.prepared.binding(
                        params![
                            context.id,
                            scope,
                            key,
                            position,
                            context.language.as_str(),
                            self.namespace.as_str(),
                            key.trim_end_matches('!'),
                            ruby_method
                        ],
                        context.language != Language::Php && self.namespace.as_str() != "macro",
                    )
                },
            )?;
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
            let parent: Option<u32> = self.connection.cached_row(
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
