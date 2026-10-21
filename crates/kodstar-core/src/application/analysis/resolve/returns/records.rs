// Purpose: Read one return annotation or fixture producer with named fields.
// Pattern: Query Record.
// Usage: Return-type lookup streams these rows from its depth-local candidate table.
// Related: crates/kodstar-core/src/application/analysis/resolve/returns/mod.rs.

use rusqlite::Row;

pub(super) const ANNOTATIONS: &str = "
    SELECT s.file_id,
           CASE WHEN b.kind='python_fixture_factory' THEN b.scope_id ELSE s.scope_id END,
           b.start_byte,b.type_name,s.language,b.imported_name,b.kind
    FROM resolution_returns r
    JOIN symbols s ON s.id=r.id
    JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id
    WHERE r.depth=?1 AND (
        (?2=0 AND b.kind='return_type')
        OR (?2 AND b.kind IN ('python_fixture_type','python_fixture_factory','python_fixture_unknown'))
    ) ORDER BY s.id,b.id";

pub(super) struct ReturnBinding {
    pub file: i64,
    pub scope: u32,
    pub position: usize,
    pub type_name: Option<String>,
    pub language: String,
    pub producer: Option<String>,
    pub kind: String,
}

impl ReturnBinding {
    pub fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            file: row.get(0)?,
            scope: row.get(1)?,
            position: row.get(2)?,
            type_name: row.get(3)?,
            language: row.get(4)?,
            producer: row.get(5)?,
            kind: row.get(6)?,
        })
    }
}
