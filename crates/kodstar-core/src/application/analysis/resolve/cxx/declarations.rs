// Purpose: 정확한 서명과 타입 소유자가 같은 C++ 선언·단일 구현 연결.
// Pattern: Simple Module.
// Usage: owner hint 해석 뒤 대표 객체 대응을 다시 계산하기 전에 실행.
// Related: crates/kodstar-core/src/application/analysis/resolve/identities.rs, crates/kodstar-core/src/infrastructure/parse/cxx/mod.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use rusqlite::{Connection, params};

pub(in crate::application::analysis::resolve) fn run(
    c: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS cxx_identities(id INTEGER PRIMARY KEY,identity TEXT,definition INTEGER); DELETE FROM cxx_identities;")?;
    let mut statement=c.prepare("SELECT s.id,owner.logical_key,s.name,b.module_name,condition.effective,b.imported_name='definition'
        FROM symbols s
        JOIN cxx_conditions condition ON condition.id=s.id
        JOIN symbols owner ON owner.id=s.owner_id
        JOIN bindings b ON b.file_id=s.file_id
        AND b.target_local=s.local_id
        WHERE s.language='cpp'
        AND owner.kind IN ('class','struct','type')
        AND b.kind='cxx_callable'
        ORDER BY s.id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let id: i64 = row.get(0)?;
        let definition: bool = row.get(5)?;
        let mut hash = blake3::Hasher::new();
        hash.update(b"cpp-member");
        for column in 1..5 {
            let part: String = row.get(column)?;
            hash.update(&(part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        c.execute(
            "INSERT INTO cxx_identities VALUES(?1,?2,?3)",
            params![id, hash.finalize().to_hex().to_string(), definition],
        )?;
    }
    c.execute("UPDATE symbols SET logical_key=(SELECT identity FROM cxx_identities WHERE id=symbols.id) WHERE id IN (SELECT id FROM cxx_identities WHERE identity IN (SELECT identity FROM cxx_identities GROUP BY identity HAVING SUM(definition)<2))",[])?;
    Ok(())
}
