// Purpose: 명시적 impl·receiver·상수 소유자의 파일 간 메서드 소유권 연결.
// Pattern: Simple Module.
// Usage: fact 해석 전에 저장된 owner_hint를 같은 lexical·import 규칙으로 처리.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/infrastructure/parse/ownership.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::params;

pub(super) fn run<'conn>(
    connection: &Connection<'conn>,
    cancel: &Cancellation,
    prepared: &crate::application::analysis::resolve::prepared::Prepared<'conn>,
) -> Result<()> {
    // Owners are derived state: remove stale cross-file owners before resolving changed imports.
    connection.cached_execute("UPDATE symbols SET owner_id=(SELECT id FROM symbols f WHERE f.file_id=symbols.file_id AND f.local_id=0) WHERE EXISTS(SELECT 1 FROM bindings b WHERE b.kind='owner_hint' AND b.file_id=symbols.file_id AND b.target_local=symbols.local_id)", [])?;
    connection.cached_execute("UPDATE symbols SET resolved_kind=NULLIF('function',syntax_kind) WHERE language='cpp' AND EXISTS(SELECT 1 FROM bindings b WHERE b.kind='owner_hint' AND b.file_id=symbols.file_id AND b.target_local=symbols.local_id)",[])?;
    let mut hints = connection.cached_prepare(
        "SELECT b.file_id,b.scope_id,b.imported_name,m.id,b.start_byte
         FROM bindings b JOIN symbols m ON m.file_id=b.file_id AND m.local_id=b.target_local
         WHERE b.kind='owner_hint' ORDER BY b.id",
    )?;
    let mut hints = hints.query([])?;
    while let Some(hint) = hints.next()? {
        cancel.check()?;
        let file: i64 = hint.get(0)?;
        let scope: u32 = hint.get(1)?;
        let name: String = hint.get(2)?;
        let method: i64 = hint.get(3)?;
        let position: usize = hint.get(4)?;
        let Some(context) = crate::application::analysis::resolve::context(connection, file, None)?
        else {
            continue;
        };
        connection.cached_execute("DELETE FROM resolution_candidates", [])?;
        let mut lookup = Lookup {
            connection,
            fact: None,
            relation: "uses_type",
            external_scope_id: 0,
            scope_sequence: 0,
            external_present: false,
            reason: None,
            rule: "explicit_impl_owner",
            source_override: None,
            member_depth: 0,
            candidate_only: false,
            language: context.language,
            namespace: crate::domain::resolve::namespaces::Namespace::usage(
                context.language,
                "uses_type",
                &name,
            ),
            receiver: false,
            ruby_side_checked: false,
            prepared,
        };
        let external = lookup.external_scope()?;
        lookup.expression(&context, scope, position, &name, 0)?;
        lookup.finish_external_scope(external, false)?;
        connection.cached_execute("DELETE FROM resolution_candidates WHERE NOT EXISTS (SELECT 1 FROM symbols WHERE id=resolution_candidates.id AND (kind IN ('class','struct','enum','type','trait','interface') OR (?1 AND kind='module')))", [matches!(context.language,Language::Cpp|Language::Ruby)])?;
        crate::application::analysis::resolve::identities::candidates(connection)?;
        let owner: Option<i64> = connection.cached_row(
            "SELECT CASE WHEN COUNT(*)=1 THEN MIN(id) END FROM resolution_candidates",
            [],
            |row| row.get(0),
        )?;
        connection.cached_execute(
            "UPDATE symbols SET owner_id=COALESCE(?1,owner_id) WHERE id=?2",
            params![owner, method],
        )?;
        if context.language == crate::domain::languages::Language::Cpp {
            connection.cached_execute("UPDATE symbols SET resolved_kind=NULLIF(CASE WHEN (SELECT kind FROM symbols WHERE id=?1) IN ('class','struct','enum','type') THEN 'method' ELSE 'function' END,syntax_kind) WHERE id=?2",params![owner,method])?;
            if let Some(owner) = owner {
                connection.cached_execute("UPDATE symbols SET resolved_qualified_name=NULLIF((SELECT qualified_name FROM symbols WHERE id=?1)||'::'||name,syntax_qualified_name) WHERE id=?2",params![owner,method])?;
                connection.cached_execute("UPDATE bindings SET type_name=(SELECT qualified_name FROM symbols WHERE id=?1) WHERE file_id=?2 AND scope_id=(SELECT local_id FROM symbols WHERE id=?3) AND name='this' AND kind='parameter'",params![owner,file,method])?;
            }
        }
        if matches!(context.language, Language::Ruby | Language::Pascal)
            && let Some(owner) = owner
        {
            connection.cached_execute("UPDATE symbols SET resolved_kind=NULLIF(CASE WHEN syntax_kind='constructor' THEN 'constructor' ELSE 'method' END,syntax_kind),resolved_qualified_name=NULLIF((SELECT qualified_name FROM symbols WHERE id=?1)||'::'||name,syntax_qualified_name) WHERE id=?2",params![owner,method])?;
            connection.cached_execute("UPDATE bindings SET type_name=(SELECT qualified_name FROM symbols WHERE id=?1) WHERE file_id=?2 AND scope_id=(SELECT local_id FROM symbols WHERE id=?3) AND lower(name)='self' AND kind='parameter'",params![owner,file,method])?;
        }
    }
    Ok(())
}
