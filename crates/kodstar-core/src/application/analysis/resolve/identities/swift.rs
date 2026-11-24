// Purpose: SwiftPM 모듈 선언의 정체성을 선택된 심볼 범위에 계산.
// Pattern: Derived State.
// Usage: identities의 전체·추가 선언 공개가 같은 규칙을 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/identities.rs, crates/kodstar-core/src/application/analysis/resolve/identities/added.rs.

use crate::{
    domain::{cancellation::Cancellation, diagnostics::Result},
    infrastructure::index::cached::Sql as Connection,
};
use rusqlite::params;

pub(super) fn run(c: &Connection, cancel: &Cancellation, added: bool) -> Result<()> {
    let source = if added { "symbol_additions" } else { "symbols" };
    let mut statement=c.cached_prepare(&format!("SELECT s.id,s.syntax_key,f.module,f.package FROM {source} s JOIN files f ON f.id=s.file_id WHERE s.language='swift' ORDER BY s.id"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let id: i64 = row.get(0)?;
        let mut hash = blake3::Hasher::new();
        hash.update(b"swift-module-definition");
        for column in [1, 2, 3] {
            let part: String = row.get(column)?;
            hash.update(&(part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        c.cached_execute(
            "UPDATE symbols SET resolved_key=NULLIF(?1,syntax_key) WHERE id=?2",
            params![hash.finalize().to_hex().to_string(), id],
        )?;
    }
    Ok(())
}
