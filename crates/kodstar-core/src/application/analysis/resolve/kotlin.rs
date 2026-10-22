// Purpose: Kotlin object 값의 invoke를 class 생성자로 잘못 확정하지 않도록 제한.
// Pattern: Simple Module.
// Usage: lexical expression이 callee 해석을 완료한 뒤 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/infrastructure/parse/kotlin/mod.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;

impl Lookup<'_, '_> {
    pub(super) fn kotlin_call_targets(&mut self) -> Result<()> {
        if self.language != Language::Kotlin {
            return Ok(());
        }
        if self.namespace == crate::domain::resolve::namespaces::Namespace::Types {
            self.connection.cached_execute("DELETE FROM resolution_candidates WHERE NOT EXISTS (SELECT 1 FROM symbols WHERE id=resolution_candidates.id AND (kind IN ('class','interface','trait','enum','struct','type')))",[])?;
            return Ok(());
        }
        if self.receiver || self.relation != "calls" {
            return Ok(());
        }
        let removed=self.connection.cached_execute("DELETE FROM resolution_candidates WHERE EXISTS(SELECT 1 FROM symbols s JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id WHERE s.id=resolution_candidates.id AND b.kind='kotlin_object_value')",[])?;
        if removed > 0 {
            self.reason = Some(
                "Kotlin object invocation requires an operator invoke target; it is not a class constructor",
            );
        }
        Ok(())
    }
}
