// Purpose: 관측된 include 범위에 없는 문장 매크로의 불완전한 구문 문맥 진단.
// Pattern: Evidence Guard.
// Usage: resolver 준비 후 미확인 매크로를 파일별 부분 분석으로 기록.
// Related: crates/kodstar-core/src/infrastructure/parse/cxx/macros.rs, crates/kodstar-core/src/application/analysis/resolve/indexed_paths.rs.

use crate::application::analysis::resolve::indexed_paths::normalize;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::{path_bytes, path_from_bytes};
use crate::infrastructure::index::cached::Sql;
use rusqlite::params;
use std::path::Path;

pub(in crate::application::analysis::resolve) fn validate(
    c: &Sql<'_>,
    cancel: &Cancellation,
) -> Result<()> {
    c.cached_execute(
        "DELETE FROM diagnostics WHERE code='source_macro_incomplete'",
        [],
    )?;
    let present: bool = c.cached_row(
        "SELECT EXISTS(SELECT 1 FROM bindings WHERE kind='native_macro_requirement')",
        [],
        |row| row.get(0),
    )?;
    if !present {
        return Ok(());
    }
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS native_macro_includes(source INTEGER,target INTEGER,PRIMARY KEY(source,target)); DELETE FROM native_macro_includes;")?;
    let mut includes = c.cached_prepare("SELECT b.file_id,f.path,b.module_name FROM bindings b JOIN files f ON f.id=b.file_id WHERE b.kind='include' AND f.language IN ('c','cpp') ORDER BY b.id")?;
    let mut rows = includes.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let file: i64 = row.get(0)?;
        let raw: Vec<u8> = row.get(1)?;
        let path = path_from_bytes(&raw)?;
        let module: String = row.get(2)?;
        let Some(raw) = module.strip_prefix("@include:") else {
            continue;
        };
        let Some(target) = normalize(path.parent().unwrap_or_else(|| Path::new("")), raw) else {
            continue;
        };
        c.cached_execute("INSERT OR IGNORE INTO native_macro_includes SELECT ?1,id FROM files WHERE path=?2 AND language IN ('c','cpp') AND status='analyzed'", params![file, path_bytes(&target)])?;
    }
    let mut requirements = c.cached_prepare("SELECT DISTINCT file_id,name_key_id,name FROM bindings WHERE kind='native_macro_requirement' ORDER BY file_id,name_key_id")?;
    let mut rows = requirements.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let file: i64 = row.get(0)?;
        let name: i64 = row.get(1)?;
        let found: bool = c.cached_row("WITH RECURSIVE included(id) AS (SELECT ?1 UNION SELECT i.target FROM native_macro_includes i JOIN included r ON r.id=i.source)
            SELECT EXISTS(SELECT 1 FROM included r JOIN bindings b ON b.file_id=r.id WHERE b.name_key_id=?2 AND b.kind='native_macro_definition')", params![file,name], |row|row.get(0))?;
        if !found {
            let spelling: String = row.get(2)?;
            c.cached_execute("INSERT INTO diagnostics(file_id,code,message) VALUES(?1,'source_macro_incomplete',?2)", params![file, format!("missing semicolon or undeclared statement macro: {spelling}; source graph is partial")])?;
        }
    }
    Ok(())
}
