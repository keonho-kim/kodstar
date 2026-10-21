// Purpose: Lisp 객체의 선언 scope로 실제 home package를 복원.
// Pattern: Storage Adapter.
// Usage: 공개 객체를 완성하는 index adapter가 호출.
// Related: crates/kodstar-core/src/infrastructure/index/mod.rs, crates/kodstar-core/src/infrastructure/parse/lisp/packages.rs.

use crate::domain::diagnostics::Result;
use crate::domain::symbols::Symbol;
use rusqlite::{Connection, OptionalExtension};

pub(super) fn attach(c: &Connection, symbol: &mut Symbol) -> Result<()> {
    if symbol.language != "commonlisp" || symbol.kind == "file" {
        return Ok(());
    }
    let package:Option<String>=c.query_row("SELECT module_name FROM bindings b JOIN symbols s ON s.file_id=b.file_id AND s.local_id=b.target_local WHERE s.id=?1 AND b.kind IN ('lisp_scope_package','lisp_pkg_def') LIMIT 1",[symbol.id],|r|r.get(0)).optional()?;
    let package = if package.is_some() {
        package
    } else {
        c.query_row("WITH RECURSIVE parents(file_id,id,depth) AS (SELECT file_id,scope_id,0 FROM symbols WHERE id=?1 UNION ALL SELECT s.file_id,s.parent,p.depth+1 FROM parents p JOIN scopes s ON s.file_id=p.file_id AND s.local_id=p.id WHERE s.parent IS NOT NULL AND p.depth<1024) SELECT b.module_name FROM parents p JOIN bindings b ON b.file_id=p.file_id AND b.scope_id=p.id AND b.kind='lisp_scope_package' ORDER BY p.depth LIMIT 1",[symbol.id],|r|r.get(0)).optional()?
    };
    if let Some(package) = package {
        symbol.module = package;
    }
    Ok(())
}
