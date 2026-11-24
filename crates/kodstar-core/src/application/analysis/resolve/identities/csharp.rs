// Purpose: C# partial·보통 선언의 정체성을 선택된 심볼 범위에 계산.
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
    let present: bool = c.cached_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {source} WHERE language='csharp')"),
        [],
        |r| r.get(0),
    )?;
    if !present {
        return Ok(());
    }
    if added {
        let root: Vec<u8> = c.cached_row("SELECT root FROM metadata", [], |r| r.get(0))?;
        let root = crate::infrastructure::cache::paths::path_from_bytes(&root)?;
        crate::application::analysis::dotnet_projects::run(c, &root, cancel)?;
    }
    let mut statement=c.cached_prepare(&format!("SELECT s.id,s.syntax_key,f.package,s.qualified_name,s.kind,s.condition,
        (SELECT b.imported_name FROM bindings b JOIN dotnet_membership d ON d.file_id=b.file_id WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='csharp_partial' AND d.merge_allowed=1 LIMIT 1)
        FROM {source} s JOIN files f ON f.id=s.file_id WHERE s.language_id=(SELECT id FROM atoms WHERE value='csharp') ORDER BY s.id"))?;
    let records = statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })?;
    for record in records {
        cancel.check()?;
        let (id, syntax, package, name, kind, condition, partial) = record?;
        let mut hash = blake3::Hasher::new();
        let parts = if let Some(declaration_kind) = partial.as_deref() {
            vec![
                "csharp-partial",
                package.as_str(),
                name.as_str(),
                kind.as_str(),
                declaration_kind,
                condition.as_deref().unwrap_or(""),
            ]
        } else {
            vec!["csharp-definition", package.as_str(), syntax.as_str()]
        };
        for part in parts {
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
