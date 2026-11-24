// Purpose: Java 이름 해석 후 인자 범위와 모순되는 후보 제거.
// Pattern: Resolver.
// Usage: lexical 해석기가 Java 호출 또는 호출 결과 producer를 처리.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/domain/resolve/java.rs.

use crate::domain::{diagnostics::Result, resolve::java::PARAMETERS};

impl super::Lookup<'_, '_> {
    pub(super) fn java_call_targets(&mut self, count: usize) -> Result<()> {
        let removed = self.connection.cached_execute(
            "DELETE FROM resolution_candidates WHERE id IN (
                SELECT c.id FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id
                JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id
                WHERE s.language='java' AND b.kind=?2
                  AND (CAST(b.imported_name AS INTEGER)>?1
                       OR (b.type_name='fixed' AND CAST(b.imported_name AS INTEGER)<>?1))
            )",
            rusqlite::params![count, PARAMETERS],
        )?;
        if removed != 0 {
            self.rule = "java_argument_count";
            self.reason = Some("other definitions have incompatible argument counts");
        }
        Ok(())
    }
}
