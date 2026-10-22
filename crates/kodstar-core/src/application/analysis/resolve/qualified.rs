// Purpose: Receiver binding·타입·namespace를 통한 한정 참조 해석.
// Pattern: Simple Module.
// Usage: lexical expression 분류기가 receiver와 member를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/application/analysis/resolve/members.rs.

use crate::application::analysis::resolve::lexical::receiver_type;
use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;

impl Lookup<'_, '_> {
    pub(super) fn qualified(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        expression: &str,
        (receiver, member): (&str, &str),
        depth: usize,
    ) -> Result<()> {
        let type_path = matches!(context.language, Language::Kotlin | Language::Scala)
            && self.namespace == crate::domain::resolve::namespaces::Namespace::Types
            || context.language == Language::Php && !expression.starts_with('$')
            || context.language == Language::Rust
                && expression
                    .rfind("::")
                    .is_some_and(|i| expression.rfind('.').is_none_or(|d| i > d));
        if !receiver.contains(['.', ':'])
            && let Some(binding) =
                self.receiver_binding(context, scope, position, receiver, type_path)?
        {
            if binding.kind == "call_result" {
                if let Some(producer) = binding.imported.as_deref() {
                    self.call_result(
                        context,
                        binding.scope,
                        binding.position,
                        producer,
                        depth + 1,
                    )?;
                    self.members(member)?;
                }
                return Ok(());
            }
            if binding.kind == "import_type"
                && matches!(self.relation, "calls" | "references" | "indirect_call")
            {
                self.reason = Some("type-only import cannot establish a runtime value");
                return Ok(());
            }
            if let Some(module) = &binding.module {
                let path = binding
                    .imported
                    .as_ref()
                    .map(|imported| format!("{imported}::{member}"));
                return self.module(
                    context,
                    binding.scope,
                    module,
                    Some(path.as_deref().unwrap_or(member)),
                    depth + 1,
                );
            }
            if let Some(type_name) = &binding.type_name {
                self.type_expression(
                    context,
                    binding.scope,
                    binding.position,
                    receiver_type(context.language, type_name),
                    depth + 1,
                )?;
                self.connection.cached_execute("DELETE FROM resolution_candidates WHERE NOT EXISTS (SELECT 1 FROM symbols WHERE id=resolution_candidates.id AND (kind IN ('class','struct','enum','type','interface','trait')))",[])?;
                return self.members(member);
            }
            if binding.target.is_none() && binding.kind != "alias" {
                if binding.kind == "parameter"
                    && self.pytest_fixture(context, binding.scope, receiver, depth + 1)?
                {
                    return self.members(member);
                }
                self.reason = Some("receiver type is not statically established");
                return Ok(());
            }
        }
        self.receiver_expression(context, scope, position, receiver, depth + 1, type_path)?;
        let count: i64 = self.connection.cached_row(
            "SELECT COUNT(*) FROM resolution_candidates",
            [],
            |row| row.get(0),
        )?;
        if count > 0 || self.has_externals() {
            return self.members(member);
        }
        if matches!(context.language, Language::Kotlin | Language::Swift) {
            let head = receiver
                .split(['.', ':'])
                .find(|s| !s.is_empty())
                .unwrap_or(receiver);
            if self.binding(context, scope, position, head)?.is_some() {
                self.reason = Some("a lexical receiver shadows the package path");
                return Ok(());
            }
        }
        if self.declared_namespace(context, receiver)? {
            if !self.candidate_only {
                self.reason = None;
            }
            return self.module(context, scope, receiver, Some(member), depth + 1);
        }
        if context.language == Language::Rust
            && matches!(
                receiver.split("::").next(),
                Some("crate" | "self" | "super")
            )
        {
            return self.module(context, scope, receiver, Some(member), depth + 1);
        }
        self.reason = Some("receiver has no identified lexical binding or namespace");
        Ok(())
    }
}
