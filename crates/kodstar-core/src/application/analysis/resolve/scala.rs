// Purpose: Scala class/object 이름 공간과 실제 object apply 호출의 대상 제한.
// Pattern: Simple Module.
// Usage: package·import 후보 선택과 lexical 호출 해석이 적용.
// Related: crates/kodstar-core/src/domain/resolve/namespaces.rs, crates/kodstar-core/src/application/analysis/resolve/members.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::resolve::namespaces::Namespace;

impl Lookup<'_, '_> {
    pub(super) fn scala_targets(&self) -> Result<()> {
        if self.language != Language::Scala || self.namespace == Namespace::Any {
            return Ok(());
        }
        if self.namespace == Namespace::Types {
            self.connection.execute("DELETE FROM resolution_candidates WHERE id IN (
                SELECT s.id FROM symbols s WHERE s.kind NOT IN ('class','trait','type','enum','struct','interface')
                OR EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='value_object'))",[])?;
        } else {
            self.connection.execute("DELETE FROM resolution_candidates WHERE id IN (
                SELECT s.id FROM symbols s WHERE s.kind IN ('class','trait','type','enum','struct','interface')
                AND NOT EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='value_object'))",[])?;
        }
        Ok(())
    }

    pub(super) fn scala_call_targets(&mut self, expression: &str) -> Result<()> {
        if self.language != Language::Scala
            || self.receiver
            || self.relation != "calls"
            || self.namespace == Namespace::Types
            || expression.starts_with("@constructor:")
        {
            return Ok(());
        }
        let all_objects:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM resolution_candidates)
            AND NOT EXISTS(SELECT 1 FROM resolution_candidates c JOIN symbols s ON s.id=c.id
              WHERE NOT EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='value_object'))",[],|r|r.get(0))?;
        if all_objects {
            self.members("apply")?;
            self.rule = "scala_object_apply";
            let found: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
                [],
                |r| r.get(0),
            )?;
            if !found {
                self.reason
                    .get_or_insert("Scala object has no identified apply member");
            }
        }
        Ok(())
    }
}
