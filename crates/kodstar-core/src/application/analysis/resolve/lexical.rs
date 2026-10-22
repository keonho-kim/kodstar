// Purpose: scope·shadowing·receiver·멤버의 대상 해석.
// Pattern: Simple Module.
// Usage: fact별 Lookup이 lexical 해석을 수행.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs.

use crate::application::analysis::resolve::builtins::builtin;
use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::resolve;
use crate::domain::resolve::Reference;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(super) fn expression(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        expression: &str,
        depth: usize,
    ) -> Result<()> {
        let frame = self.external_scope()?;
        self.expression_inner(context, scope, position, expression, depth)?;
        self.kotlin_call_targets()?;
        self.scala_call_targets(expression)?;
        self.finish_external_scope(frame, true)
    }

    fn expression_inner(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        expression: &str,
        depth: usize,
    ) -> Result<()> {
        if depth >= 32 {
            self.reason = Some("alias or reexport cycle/depth limit");
            return Ok(());
        }
        if let Some(constructor) = expression.strip_prefix("@constructor:") {
            self.type_expression(context, scope, position, constructor, depth + 1)?;
            self.connection.cached_execute("DELETE FROM resolution_candidates WHERE NOT EXISTS (SELECT 1 FROM symbols WHERE id=resolution_candidates.id AND (kind IN ('class','struct','enum','type')))",[])?;
            self.ruby_constructor()?;
            return Ok(());
        }
        if let Some(result) = expression.strip_prefix("@result:") {
            if let Some((producer, member)) = result.rsplit_once('#') {
                self.call_result(context, scope, position, producer, depth + 1)?;
                self.members(member)?;
            }
            return Ok(());
        }
        if let Some(member) = expression.strip_prefix("@super:") {
            return self.python_super(context, scope, position, member);
        }
        if context.language == Language::Php
            && self.php_expression(context, scope, position, expression, depth)?
        {
            return Ok(());
        }
        match resolve::reference(context.language, expression) {
            Reference::Dynamic => {
                self.reason = Some("dynamic callee or receiver expression");
                Ok(())
            }
            Reference::Bare(name) => self.bare(context, scope, position, &name, depth),
            Reference::LispPackage {
                package,
                symbol,
                external_only,
            } => self.lisp_qualified(context, scope, position, &package, &symbol, external_only),
            Reference::Qualified { receiver, member } => self.qualified(
                context,
                scope,
                position,
                expression,
                (&receiver, &member),
                depth,
            ),
        }
    }

    pub(super) fn bare(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        name: &str,
        depth: usize,
    ) -> Result<()> {
        let key = context.language.lookup_key(name);
        if let Some(binding) = self.binding(context, scope, position, name)? {
            if binding.kind == "import_type"
                && matches!(self.relation, "calls" | "references" | "indirect_call")
                && !self.receiver
            {
                self.reason = Some("type-only import cannot establish a runtime value");
                return Ok(());
            }
            if binding.target.is_some() {
                self.rule = "lexical_binding";
                self.prepared.candidates(params![
                    context.id,
                    binding.scope,
                    key,
                    self.namespace.as_str(),
                    key.trim_end_matches('!')
                ])?;
                self.rust_targets()?;
                self.cxx_targets()?;
            } else if let Some(module) = binding.module {
                self.rule = "explicit_import";
                self.module(
                    context,
                    binding.scope,
                    &module,
                    binding.imported.as_deref(),
                    depth + 1,
                )?;
            } else if matches!(binding.kind.as_str(), "alias" | "rust_self_constructor") {
                if let Some(target) = binding.imported {
                    self.expression(context, binding.scope, binding.position, &target, depth + 1)?;
                }
            } else {
                self.reason =
                    Some("lexical binding shadows other definitions without an identified target");
            }
            return Ok(());
        }
        if context.language == Language::CommonLisp {
            return self.lisp_global(context, scope, &key);
        }
        if self.namespace_peers(context, scope, &key)? {
            return Ok(());
        }
        if context.language == Language::Ruby
            && matches!(self.relation, "calls" | "references")
            && name.chars().next().is_some_and(char::is_lowercase)
            && self.ruby_implicit(context, scope, name)?
        {
            return Ok(());
        }
        if matches!(
            context.language,
            Language::Java | Language::Kotlin | Language::Scala | Language::Swift
        ) && self.package_peers(context, &key)?
        {
            return Ok(());
        }
        if self.scope_imports(context, scope, name, depth)? {
            return Ok(());
        }
        if self.rust_implicit_exported_macro(context, scope, name)? {
            return Ok(());
        }
        if context.language == Language::Ruby && matches!(self.relation, "calls" | "references") {
            self.ruby_implicit(context, scope, name)?;
            return Ok(());
        }
        if context.language == Language::Go {
            self.package_peers(context, &key)?;
        }
        let found: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |row| row.get(0),
        )?;
        self.dependency(
            "module_name",
            &format!("{}:{}:{key}", context.package, context.module),
            found,
        )?;
        if !found && builtin(context.language, name, self.relation) {
            self.external_target(&format!("builtin:{}:{name}", context.language.as_str()))?;
            self.rule = "language_builtin";
        }
        Ok(())
    }
}

pub(super) fn receiver_type(language: Language, raw: &str) -> &str {
    let raw = raw.trim_matches(['&', '*']).trim();
    let raw = raw.strip_prefix("dyn ").unwrap_or(raw);
    if matches!(
        language,
        Language::Swift | Language::Kotlin | Language::CSharp
    ) {
        raw.trim_end_matches(['?', '!'])
    } else if matches!(language, Language::Go | Language::Scala) {
        raw.split('[').next().unwrap_or(raw)
    } else {
        raw
    }
}
