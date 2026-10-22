// Purpose: 실제 함수 값 참조를 확정하고 실행을 증명하지 못한 간접 호출을 후보로 제한.
// Pattern: Simple Module.
// Usage: fact 대상 해석 후 공개 전에 실제 callable 후보만 남김.
// Related: crates/kodstar-core/src/infrastructure/parse/callable_values.rs, crates/kodstar-core/src/application/analysis/resolve/mod.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::{OptionalExtension, params};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ValueOrigin {
    Other,
    Name,
    Getattr,
    Unproven,
}

impl Lookup<'_, '_> {
    pub(super) fn callable_value_origin(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
    ) -> Result<ValueOrigin> {
        if !matches!(
            self.language,
            Language::Python
                | Language::JavaScript
                | Language::TypeScript
                | Language::Elixir
                | Language::Erlang
        ) || !matches!(self.relation, "references" | "indirect_call")
        {
            return Ok(ValueOrigin::Other);
        }
        let marked:Option<bool>=self.connection.cached_row("SELECT b.module_name='getattr' FROM bindings b JOIN facts f ON f.file_id=b.file_id
            WHERE f.id=?1 AND b.scope_id=?2 AND b.name_key='@callable-value' AND b.start_byte=?3 AND b.kind='callable_value' LIMIT 1",params![self.fact,scope,position],|r|r.get(0)).optional()?;
        match marked {
            None => Ok(ValueOrigin::Other),
            Some(false) => Ok(ValueOrigin::Name),
            Some(true) => {
                let frame = self.external_scope()?;
                self.expression(context, scope, position, "getattr", 0)?;
                let builtin:bool=self.connection.cached_row("SELECT
                    NOT EXISTS(SELECT 1 FROM resolution_candidates)
                    AND EXISTS(SELECT 1 FROM resolution_externals WHERE scope=?1 AND key='builtin:python:getattr')
                    AND NOT EXISTS(SELECT 1 FROM resolution_externals WHERE scope=?1 AND key<>'builtin:python:getattr')
                    AND NOT EXISTS(SELECT 1 FROM resolution_dependencies WHERE fact_id=?2 AND kind LIKE 'scope:%' AND lookup_key GLOB '*:getattr' AND found=1)",params![self.external_scope_id,self.fact],|r|r.get(0))?;
                self.connection
                    .cached_execute("DELETE FROM resolution_candidates", [])?;
                self.finish_external_scope(frame, false)?;
                self.reason = None;
                self.candidate_only = false;
                self.rule = "lexical_binding";
                self.dependency("getattr_origin", "getattr", builtin)?;
                Ok(if builtin {
                    ValueOrigin::Getattr
                } else {
                    ValueOrigin::Unproven
                })
            }
        }
    }

    pub(super) fn callable_value_targets(
        &mut self,
        origin: ValueOrigin,
        expression: &str,
    ) -> Result<bool> {
        if origin == ValueOrigin::Other {
            return Ok(true);
        }
        let getattr = origin == ValueOrigin::Getattr;
        let previous: u64 =
            self.connection
                .cached_row("SELECT COUNT(*) FROM resolution_candidates", [], |r| {
                    r.get(0)
                })?;
        self.connection.cached_execute("DELETE FROM resolution_candidates WHERE NOT EXISTS (SELECT 1 FROM symbols WHERE id=resolution_candidates.id AND (kind IN ('function','method')))",[])?;
        if self.relation == "indirect_call" {
            self.connection.cached_execute("DELETE FROM resolution_candidates WHERE id IN (SELECT s.id FROM symbols s JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id WHERE b.kind IN ('python_property','callable_accessor'))",[])?;
        }
        let count: u64 =
            self.connection
                .cached_row("SELECT COUNT(*) FROM resolution_candidates", [], |r| {
                    r.get(0)
                })?;
        self.dependency("callable_value", expression, count > 0)?;
        if count == 0 {
            self.clear_externals()?;
            return Ok(false);
        }
        if self.relation == "indirect_call" {
            self.candidate_only = true;
            self.rule = "indirect_function_value";
            self.reason = Some("function value may be invoked; execution is not established");
        } else {
            let accessor:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id WHERE b.kind IN ('python_property','callable_accessor'))",[],|r|r.get(0))?;
            self.rule = if accessor {
                "accessor_value_origin_reference"
            } else if getattr {
                "explicit_static_getattr_reference"
            } else {
                "explicit_function_value_reference"
            };
            if count < previous || self.has_externals() {
                self.candidate_only = true;
                self.reason =
                    Some("some value targets have no established internal callable definition");
            }
        }
        Ok(true)
    }
}
