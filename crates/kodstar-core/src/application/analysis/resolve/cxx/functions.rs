// Purpose: 동일 서명·include 문맥의 C/C++ free function 선언과 단일 정의 연결.
// Pattern: Simple Module.
// Usage: 소유자·메서드 선언 연결 후 free function 그룹을 처리.
// Related: crates/kodstar-core/src/application/analysis/resolve/cxx/mod.rs, crates/kodstar-core/src/infrastructure/parse/cxx/mod.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use rusqlite::{Connection, params};

pub(in crate::application::analysis::resolve) fn run(
    c: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    crate::application::analysis::resolve::cxx::stage(c)?;
    let mut statement=c.prepare("SELECT s.id,s.file_id,s.language,f.package,s.qualified_name,b.module_name,condition.effective,b.imported_name='definition',CASE WHEN b.type_name='internal' THEN f.path ELSE X'' END
        FROM symbols s JOIN cxx_conditions condition ON condition.id=s.id JOIN files f ON f.id=s.file_id JOIN symbols owner ON owner.id=s.owner_id
        JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id
        WHERE s.language IN ('c','cpp') AND b.kind='cxx_callable' AND s.kind='function' AND owner.kind IN ('file','module')
          AND (owner.kind='module' OR NOT EXISTS(SELECT 1 FROM bindings h WHERE h.file_id=s.file_id AND h.target_local=s.local_id AND h.kind='owner_hint')) ORDER BY s.id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let id: i64 = row.get(0)?;
        let file: i64 = row.get(1)?;
        let definition: bool = row.get(7)?;
        let mut hash = blake3::Hasher::new();
        hash.update(b"cxx-free-function");
        for column in 2..7 {
            let part: String = row.get(column)?;
            hash.update(&(part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        let local: Vec<u8> = row.get(8)?;
        hash.update(&(local.len() as u64).to_le_bytes());
        hash.update(&local);
        c.execute(
            "INSERT INTO cxx_rows VALUES(?1,?2,?3,?4)",
            params![id, file, hash.finalize().to_hex().to_string(), definition],
        )?;
    }
    crate::application::analysis::resolve::cxx::merge(c, cancel)
}
