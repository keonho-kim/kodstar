// Purpose: Python global·nonlocal 지시에 맞는 바인딩 scope 선택.
// Pattern: Simple Module.
// Usage: lexical binding이 각 Python scope를 탐색하기 전에 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/infrastructure/parse/python/mod.rs.

pub(super) mod exports;
pub(super) mod fixtures;
pub(super) mod mro;
pub(super) mod properties;

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::{OptionalExtension, params};

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn python_scope(
        &self,
        context: &FileContext,
        scope: u32,
        key: &str,
    ) -> Result<Option<Option<u32>>> {
        if context.language != Language::Python || scope == 0 {
            return Ok(None);
        }
        let directive:Option<String>=self.connection.query_row(
            "SELECT kind FROM bindings WHERE file_id=?1 AND scope_id=?2 AND name_key=?3 AND kind IN ('python_global','python_nonlocal') LIMIT 1",params![context.id,scope,key],|r|r.get(0)).optional()?;
        match directive.as_deref() {
            Some("python_global") => Ok(Some(Some(0))),
            Some("python_nonlocal") => {
                let scope:Option<u32>=self.connection.query_row(
                    "WITH RECURSIVE parents(id,parent,depth) AS (
                       SELECT local_id,parent,0 FROM scopes WHERE file_id=?1 AND local_id=?2
                       UNION ALL SELECT s.local_id,s.parent,p.depth+1 FROM parents p JOIN scopes s ON s.file_id=?1 AND s.local_id=p.parent WHERE p.depth<1024
                     ) SELECT p.id FROM parents p JOIN symbols s ON s.file_id=?1 AND s.local_id=p.id
                       WHERE p.depth>0 AND s.kind IN ('function','method') AND EXISTS(
                         SELECT 1 FROM bindings b WHERE b.file_id=?1 AND b.scope_id=p.id AND b.name_key=?3 AND b.kind NOT IN ('python_global','python_nonlocal')
                       ) ORDER BY p.depth LIMIT 1",params![context.id,scope,key],|r|r.get(0)).optional()?;
                Ok(Some(scope))
            }
            _ => Ok(None),
        }
    }
}
