// Purpose: elixir 심볼 정체성을 전체 또는 추가된 선언 범위에서 계산.
// Pattern: Derived State.
// Usage: identities의 전체·삽입 공개 준비가 동일 규칙을 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/identities.rs, crates/kodstar-core/src/application/analysis/resolve/identities/added.rs.

use crate::{
    domain::{cancellation::Cancellation, diagnostics::Result},
    infrastructure::index::cached::Sql as Connection,
};
use rusqlite::params;

pub(super) fn run(c: &Connection, cancel: &Cancellation, added: bool) -> Result<()> {
    let source = if added { "symbol_additions" } else { "symbols" };
    let mut statement=c.cached_prepare(&format!("SELECT s.id,f.package,f.path,s.qualified_name,s.kind,COALESCE(s.condition,''),b.module_name
        FROM {source} s
        JOIN files f ON f.id=s.file_id
        JOIN bindings b ON b.file_id=s.file_id
        AND b.target_local=s.local_id
        WHERE s.language_id=(SELECT id FROM atoms WHERE value='elixir')
        AND b.kind='elixir_function'
        ORDER BY s.id"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let id: i64 = row.get(0)?;
        let mut hash = blake3::Hasher::new();
        hash.update(b"elixir-function");
        for column in [1, 3, 4, 5, 6] {
            let part: String = row.get(column)?;
            hash.update(&(part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        let path: Vec<u8> = row.get(2)?;
        hash.update(&(path.len() as u64).to_le_bytes());
        hash.update(&path);
        c.cached_execute(
            "UPDATE symbols SET resolved_key=NULLIF(?1,syntax_key) WHERE id=?2",
            params![hash.finalize().to_hex().to_string(), id],
        )?;
    }
    Ok(())
}
