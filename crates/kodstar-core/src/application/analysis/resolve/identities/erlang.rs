// Purpose: erlang 심볼 정체성을 전체 또는 추가된 선언 범위에서 계산.
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
    c.cached_execute(&format!("UPDATE symbols SET exported=EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=symbols.file_id AND b.kind='erlang_export' AND b.name IN (symbols.name,'*')) WHERE language='erlang' AND kind='function'{}", if added { " AND id IN (SELECT id FROM symbol_additions)" } else { "" }),[])?;
    c.cached_execute(&format!("UPDATE symbols SET exported=EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=symbols.file_id AND b.kind='erlang_type_export' AND b.name=symbols.name) WHERE language='erlang' AND kind='type'{}", if added { " AND id IN (SELECT id FROM symbol_additions)" } else { "" }),[])?;
    let mut statement=c.cached_prepare(&format!("SELECT s.id,f.package,f.module,f.path,s.qualified_name,s.kind,COALESCE(s.condition,'') FROM {source} s JOIN files f ON f.id=s.file_id WHERE s.language_id=(SELECT id FROM atoms WHERE value='erlang') AND s.kind='function' ORDER BY s.id"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let id: i64 = row.get(0)?;
        let mut hash = blake3::Hasher::new();
        for column in [1, 2, 4, 5, 6] {
            let part: String = row.get(column)?;
            hash.update(&(part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        let path: Vec<u8> = row.get(3)?;
        hash.update(&(path.len() as u64).to_le_bytes());
        hash.update(&path);
        c.cached_execute(
            "UPDATE symbols SET resolved_key=NULLIF(?1,syntax_key) WHERE id=?2",
            params![hash.finalize().to_hex().to_string(), id],
        )?;
    }
    Ok(())
}
