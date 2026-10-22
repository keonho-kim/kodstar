// Purpose: 재귀 한도와 별개인 lookup 분기의 후보·module snapshot 수명 소유.
// Pattern: Scoped Disk Staging.
// Usage: module·재수출·다중 Python 파일 조회가 고유 scope를 빌리고 복원.
// Related: crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs, crates/kodstar-core/src/application/analysis/resolve/modules/exports.rs, crates/kodstar-core/src/application/analysis/resolve/python/exports.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::params;

pub(in crate::application::analysis::resolve) fn reset(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS resolution_module_snapshots(lookup_scope INTEGER,id INTEGER,PRIMARY KEY(lookup_scope,id));
        CREATE TEMP TABLE IF NOT EXISTS resolution_owner_snapshots(lookup_scope INTEGER,file_id INTEGER,owner_id INTEGER,PRIMARY KEY(lookup_scope,file_id,owner_id));
        DELETE FROM resolution_saved; DELETE FROM resolution_module_snapshots; DELETE FROM resolution_owner_snapshots;")?;
    Ok(())
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn save_candidates(&mut self) -> Result<usize> {
        let scope = self.allocate_scope()?;
        self.connection.cached_execute(
            "INSERT INTO resolution_saved SELECT ?1,id FROM resolution_candidates",
            [scope],
        )?;
        self.connection
            .cached_execute("DELETE FROM resolution_candidates", [])?;
        Ok(scope)
    }

    pub(in crate::application::analysis::resolve) fn restore_candidates(
        &self,
        scope: usize,
    ) -> Result<()> {
        self.connection.cached_execute("INSERT OR IGNORE INTO resolution_candidates SELECT id FROM resolution_saved WHERE lookup_scope=?1",[scope])?;
        self.connection.cached_execute(
            "DELETE FROM resolution_saved WHERE lookup_scope=?1",
            [scope],
        )?;
        Ok(())
    }

    pub(in crate::application::analysis::resolve) fn snapshot_modules(
        &mut self,
        depth: usize,
    ) -> Result<usize> {
        let scope = self.allocate_scope()?;
        self.connection.cached_execute("INSERT INTO resolution_module_snapshots SELECT ?1,id FROM resolution_modules WHERE depth=?2",params![scope,depth])?;
        self.connection.cached_execute("INSERT INTO resolution_owner_snapshots SELECT ?1,file_id,owner_id FROM resolution_module_owners WHERE depth=?2",params![scope,depth])?;
        Ok(scope)
    }

    pub(in crate::application::analysis::resolve) fn release_modules(
        &self,
        scope: usize,
    ) -> Result<()> {
        self.connection.cached_execute(
            "DELETE FROM resolution_module_snapshots WHERE lookup_scope=?1",
            [scope],
        )?;
        self.connection.cached_execute(
            "DELETE FROM resolution_owner_snapshots WHERE lookup_scope=?1",
            [scope],
        )?;
        Ok(())
    }
}
