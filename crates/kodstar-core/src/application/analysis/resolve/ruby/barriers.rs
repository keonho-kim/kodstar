// Purpose: 작업 내 불변인 Ruby 표식 유무와 후보 의존 소유자 검사를 분리.
// Pattern: Simple Module.
// Usage: ruby_members·ruby_dispatch가 같은 Prepared 수명에서 판정.
// Related: crates/kodstar-core/src/application/analysis/resolve/ruby/barriers.sql, crates/kodstar-core/src/application/analysis/resolve/prepared.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;

#[derive(Clone, Copy)]
pub(in crate::application::analysis::resolve) enum Barriers {
    Absent,
    Scoped,
    Project,
}

impl Lookup<'_, '_> {
    pub(super) fn ruby_barred(&self) -> Result<bool> {
        let barriers = match self.prepared.ruby_barriers.get() {
            Some(barriers) => barriers,
            None => {
                let mode:i64=self.connection.cached_row("SELECT CASE
                    WHEN EXISTS(SELECT 1 FROM bindings_rows WHERE kind=(SELECT id FROM atoms WHERE value='ruby_project_barrier')) THEN 2
                    WHEN EXISTS(SELECT 1 FROM bindings_rows WHERE kind=(SELECT id FROM atoms WHERE value='ruby_lookup_barrier')) THEN 1
                    ELSE 0 END",[],|r|r.get(0))?;
                let barriers = match mode {
                    2 => Barriers::Project,
                    1 => Barriers::Scoped,
                    _ => Barriers::Absent,
                };
                // Raw barrier kinds do not change during preparation or resolution.
                // Prepared is discarded before the next update, including negative results.
                self.prepared.ruby_barriers.set(Some(barriers));
                barriers
            }
        };
        match barriers {
            Barriers::Absent => Ok(false),
            Barriers::Project => Ok(true),
            Barriers::Scoped => {
                Ok(self
                    .connection
                    .cached_row(include_str!("barriers.sql"), [self.fact], |r| r.get(0))?)
            }
        }
    }
}
