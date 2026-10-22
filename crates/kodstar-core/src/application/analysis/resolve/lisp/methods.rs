// Purpose: generic binding·적용성을 증명하지 못한 Lisp method 참조를 후보로 표시.
// Pattern: Simple Module.
// Usage: fact 결과 공개 전에 Lisp 함수 공간 후보를 확인.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/infrastructure/parse/lisp/bindings.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn lisp_method_candidates(
        &mut self,
    ) -> Result<()> {
        if self.language != Language::CommonLisp || !matches!(self.relation, "calls" | "references")
        {
            return Ok(());
        }
        let method:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id WHERE s.kind='method')",[],|r|r.get(0))?;
        if method {
            self.candidate_only = true;
            self.rule = "lisp_method_name_candidate";
            self.reason = Some(
                "generic binding is inferred from method names; method selection is not proven",
            );
        }
        Ok(())
    }
}
