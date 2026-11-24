// Purpose: C++ namespace의 정규 qualified 이름 ID로 선언·소유 파일을 선택.
// Pattern: Indexed Query Adapter.
// Usage: namespace resolver가 C++의 정규 이름·package·lookup depth를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/namespaces.rs, crates/kodstar-core/src/infrastructure/index/records/symbols.sql.

use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql;
use rusqlite::params;

pub(super) fn declared(c: &Sql<'_>, package: &str, name: &str) -> Result<bool> {
    Ok(c.cached_row(
        "SELECT EXISTS(SELECT 1 FROM symbols_rows s CROSS JOIN files_rows f ON f.id=s.file_id
        WHERE s.qualified_id=(SELECT id FROM atoms WHERE value=?2)
        AND s.language=(SELECT id FROM atoms WHERE value='cpp')
        AND s.kind_value=(SELECT id FROM atoms WHERE value='module')
        AND f.package=(SELECT id FROM atoms WHERE value=?1))",
        params![package, name],
        |row| row.get(0),
    )?)
}

pub(super) fn owners(c: &Sql<'_>, package: &str, name: &str, depth: usize) -> Result<()> {
    c.cached_execute(
        "INSERT OR IGNORE INTO resolution_module_owners
        SELECT ?3,s.file_id,s.id FROM symbols_rows s CROSS JOIN files_rows f ON f.id=s.file_id
        WHERE s.qualified_id=(SELECT id FROM atoms WHERE value=?2)
        AND s.language=(SELECT id FROM atoms WHERE value='cpp')
        AND s.kind_value=(SELECT id FROM atoms WHERE value='module')
        AND f.package=(SELECT id FROM atoms WHERE value=?1)
        AND f.status=(SELECT id FROM atoms WHERE value='analyzed')",
        params![package, name, depth],
    )?;
    Ok(())
}
