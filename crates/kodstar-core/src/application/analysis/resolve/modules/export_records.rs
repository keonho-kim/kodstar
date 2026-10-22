// Purpose: 명시 export SQL 레코드와 단일 후보의 제한된 읽기.
// Pattern: Simple Module.
// Usage: export 해석 정책이 재수출 행이나 단일 행을 빌려 사용.
// Related: crates/kodstar-core/src/application/analysis/resolve/modules/exports.rs, crates/kodstar-core/src/application/analysis/resolve/modules/scopes.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use rusqlite::{Row, params};

pub(in crate::application::analysis::resolve) struct Export {
    pub(in crate::application::analysis::resolve) file: i64,
    pub(in crate::application::analysis::resolve) scope: u32,
    pub(in crate::application::analysis::resolve) module: Option<String>,
    pub(in crate::application::analysis::resolve) imported: Option<String>,
    pub(in crate::application::analysis::resolve) position: usize,
    pub(in crate::application::analysis::resolve) kind: String,
    pub(in crate::application::analysis::resolve) name: String,
    pub(in crate::application::analysis::resolve) condition: Option<String>,
}
impl Export {
    pub(in crate::application::analysis::resolve) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            file: row.get(0)?,
            scope: row.get(1)?,
            module: row.get(2)?,
            imported: row.get(3)?,
            position: row.get(4)?,
            kind: row.get(5)?,
            name: row.get(6)?,
            condition: row.get(7)?,
        })
    }
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn single_export(
        &self,
        head: &str,
        rust_key: Option<&str>,
        depth: usize,
    ) -> Result<Option<Export>> {
        let file:Option<i64>=self.connection.cached_row("SELECT CASE WHEN COUNT(*)=1 AND NOT EXISTS(SELECT 1 FROM resolution_module_owners WHERE depth=?1) THEN MIN(id) END FROM resolution_modules WHERE depth=?1",[depth],|r|r.get(0))?;
        let Some(file) = file else {
            return Ok(None);
        };
        let mut statement=self.connection.cached_prepare("SELECT file_id,scope_id,module_name,imported_name,start_byte,kind,name,type_name FROM bindings
            WHERE file_id=?1 AND kind IN ('export','export_type','commonjs_export_module','commonjs_export_exports') AND (name=?2 OR name_key=?3) ORDER BY id LIMIT 2")?;
        let mut records = statement.query_map(params![file, head, rust_key], Export::read)?;
        let first = records.next().transpose()?;
        if records.next().transpose()?.is_some() {
            return Ok(None);
        }
        Ok(first.filter(|record| record.scope == 0))
    }
}
