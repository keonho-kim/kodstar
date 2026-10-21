// Purpose: Swift Environment property 타입을 실제 SwiftUI import 근거로 제한.
// Pattern: Simple Module.
// Usage: 필드 타입 변환이 wrapper metadata를 만났을 때 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/field_types.rs, crates/kodstar-core/src/infrastructure/parse/swift/properties.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn swift_environment(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        depth: usize,
    ) -> Result<bool> {
        let frame = self.external_scope()?;
        self.type_expression(context, scope, position, "Environment", depth + 1)?;
        let established: bool = self.connection.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM resolution_candidates)
             AND EXISTS(SELECT 1 FROM resolution_externals WHERE scope=?1 AND key=?2)
             AND NOT EXISTS(SELECT 1 FROM resolution_externals WHERE scope=?1 AND key<>?2)",
            params![self.external_scope_id, "swift:module:SwiftUI::Environment"],
            |r| r.get(0),
        )?;
        self.connection
            .execute("DELETE FROM resolution_candidates", [])?;
        self.finish_external_scope(frame, false)?;
        Ok(established)
    }
}
