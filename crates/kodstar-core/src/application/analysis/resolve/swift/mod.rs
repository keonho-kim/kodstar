// Purpose: 정적 SwiftPM target의 명시 dependency로 로컬 module import 제한.
// Pattern: Simple Module.
// Usage: Swift module 해석이 일반 package 조회 전에 호출.
// Related: crates/kodstar-core/src/application/analysis/swift_projects.rs, crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs.

pub(super) mod properties;

use crate::application::analysis::resolve::modules::Location;
use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn swift_modules(
        &mut self,
        context: &FileContext,
        module: &str,
        depth: usize,
    ) -> Result<Location> {
        self.connection.execute(
            "DELETE FROM resolution_module_owners WHERE depth=?1",
            [depth],
        )?;
        let (manifest, target): (Option<i64>, Option<String>) = self.connection.query_row(
            "SELECT manifest,target FROM swift_membership WHERE file_id=?1",
            [context.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let Some(manifest) = manifest else {
            return Ok(Location::Unmapped);
        };
        let known:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND kind='swift_target' AND name=?2)",params![manifest,module],|r|r.get(0))?;
        if !known {
            return Ok(Location::Unmapped);
        }
        let (direct,conditional):(bool,bool)=self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND name=?2 AND module_name=?3 AND kind='swift_target_dependency'),
                    EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND name=?2 AND module_name=?3 AND kind='swift_target_dependency_conditional')",
            params![manifest,target,module],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if target.as_deref() != Some(module) && !direct && !conditional {
            self.reason = Some("Swift module is not a declared target dependency");
            return Ok(Location::Project);
        }
        if !direct && conditional && target.as_deref() != Some(module) {
            self.candidate_only = true;
            self.reason = Some("Swift target dependency has an unevaluated platform condition");
        }
        self.connection.execute("INSERT OR IGNORE INTO resolution_modules SELECT ?3,m.file_id FROM swift_membership m JOIN files f ON f.id=m.file_id WHERE m.manifest=?1 AND m.target=?2 AND f.status='analyzed'",params![manifest,module,depth])?;
        Ok(Location::Project)
    }
}
