// Purpose: 명시적 C# partial 정의의 논리 키·대표 객체 대응 계산.
// Pattern: Simple Module.
// Usage: 매 갱신에서 lexical 해석 전에 실행하며 원래 구문 행은 보존.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/infrastructure/index/schema.sql.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::params;

pub(super) fn run(c: &Connection, cancel: &Cancellation) -> Result<()> {
    c.execute_batch("DELETE FROM symbol_aliases; UPDATE symbols SET resolved_key=NULL,resolved_qualified_name=NULL,resolved_kind=NULL WHERE resolved_key IS NOT NULL OR resolved_qualified_name IS NOT NULL OR resolved_kind IS NOT NULL;")?;
    let mut statement=c.cached_prepare("SELECT s.id,s.syntax_key,f.package,s.qualified_name,s.kind,s.condition,
        (SELECT b.imported_name FROM bindings b JOIN dotnet_membership d ON d.file_id=b.file_id WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='csharp_partial' AND d.merge_allowed=1 LIMIT 1)
        FROM symbols s JOIN files f ON f.id=s.file_id WHERE s.language='csharp' ORDER BY s.id")?;
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
    erlang(c, cancel)?;
    elixir(c, cancel)?;
    crate::application::analysis::swift_projects::identities(c, cancel)?;
    rebuild(c)?;
    c.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS resolution_canonical(id INTEGER PRIMARY KEY);",
    )?;
    Ok(())
}

pub(super) fn rebuild(c: &Connection) -> Result<()> {
    c.execute_batch(
        "DELETE
        FROM symbol_aliases; INSERT INTO symbol_aliases
        SELECT symbol_id,canonical_id FROM (SELECT s.id AS symbol_id,(SELECT candidate.id
        FROM symbols candidate
        JOIN files f ON f.id=candidate.file_id
        WHERE candidate.key_blob=s.key_blob
        ORDER BY f.path,candidate.start_byte,candidate.id LIMIT 1) AS canonical_id
        FROM symbols s) WHERE symbol_id<>canonical_id;",
    )?;
    Ok(())
}

fn erlang(c: &Connection, cancel: &Cancellation) -> Result<()> {
    c.cached_execute("UPDATE symbols SET exported=EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=symbols.file_id AND b.kind='erlang_export' AND b.name IN (symbols.name,'*')) WHERE language='erlang' AND kind='function'",[])?;
    c.cached_execute("UPDATE symbols SET exported=EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=symbols.file_id AND b.kind='erlang_type_export' AND b.name=symbols.name) WHERE language='erlang' AND kind='type'",[])?;
    let mut statement=c.cached_prepare("SELECT s.id,f.package,f.module,f.path,s.qualified_name,s.kind,COALESCE(s.condition,'') FROM symbols s JOIN files f ON f.id=s.file_id WHERE s.language='erlang' AND s.kind='function' ORDER BY s.id")?;
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

fn elixir(c: &Connection, cancel: &Cancellation) -> Result<()> {
    let mut statement=c.cached_prepare("SELECT s.id,f.package,f.path,s.qualified_name,s.kind,COALESCE(s.condition,''),b.module_name
        FROM symbols s
        JOIN files f ON f.id=s.file_id
        JOIN bindings b ON b.file_id=s.file_id
        AND b.target_local=s.local_id
        WHERE s.language='elixir'
        AND b.kind='elixir_function'
        ORDER BY s.id")?;
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

pub(super) fn candidates(c: &Connection) -> Result<()> {
    c.execute_batch(
        "DELETE
        FROM resolution_canonical; INSERT
        OR IGNORE INTO resolution_canonical
        SELECT g.canonical_id
        FROM resolution_candidates r
        JOIN symbol_identities g ON g.symbol_id=r.id; DELETE
        FROM resolution_candidates; INSERT INTO resolution_candidates
        SELECT id
        FROM resolution_canonical;",
    )?;
    Ok(())
}

pub(super) fn relationships(c: &Connection) -> Result<()> {
    c.execute_batch("UPDATE relationship_rows SET source_id=(SELECT canonical_id FROM symbol_identities WHERE symbol_id=relationship_rows.source_id),target_id=(SELECT canonical_id FROM symbol_identities WHERE symbol_id=relationship_rows.target_id);")?;
    Ok(())
}
