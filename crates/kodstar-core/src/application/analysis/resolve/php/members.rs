// Purpose: PHP 메서드 대소문자와 class·trait·parent 우선순위 적용.
// Pattern: Simple Module.
// Usage: 일반 멤버 해석기가 PHP receiver 후보를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/members.rs, crates/kodstar-core/src/application/analysis/resolve/php/members.sql.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::params;

pub(in crate::application::analysis::resolve) fn reset(c: &Connection) -> Result<()> {
    c.execute_batch(include_str!("traits.sql"))?;
    Ok(())
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn php_members(
        &mut self,
        name: &str,
    ) -> Result<()> {
        self.php_prepare_traits()?;
        self.connection
            .cached_execute("DELETE FROM resolution_members", [])?;
        self.connection.cached_execute(
            include_str!("members.sql"),
            params![name, self.namespace.as_str(), self.fact],
        )?;
        self.connection.execute_batch("DELETE FROM resolution_candidates; INSERT INTO resolution_candidates SELECT id FROM resolution_members;")?;
        self.rule = "php_class_trait_parent_member";
        Ok(())
    }
}
