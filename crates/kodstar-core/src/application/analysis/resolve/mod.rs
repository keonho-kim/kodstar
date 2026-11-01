// Purpose: 저장된 fact 순회와 해석 결과·의존성 저장.
// Pattern: Coordinator.
// Usage: 세대 공개 전 analysis가 호출.
// Related: crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/src/application/analysis/resolve/lexical.rs.

//! Resolve stored facts and publish their lookup dependencies in the same transaction.
mod bindings;
mod builtins;
mod callable_values;
mod cxx;
mod dispatch;
mod ejs;
mod external_scopes;
mod fact_batch;
mod facts;
mod field_types;
mod go;
mod identities;
pub(super) mod indexed_paths;
mod inputs;
mod javascript;
mod kotlin;
mod lexical;
mod lisp;
mod lookup_context;
mod members;
mod modules;
mod namespaces;
mod owners;
mod pascal_declarations;
mod php;
mod preparation;
mod prepared;
mod publication;
mod python;
mod qualified;
mod razor;
mod returns;
mod ruby;
mod rust;
mod scala;
mod scope_imports;
mod swift;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::cache::paths::path_from_bytes;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::params;
use std::path::PathBuf;

struct FileContext {
    id: i64,
    language: Language,
    input_format: Option<Language>,
    module: String,
    package: String,
    path: PathBuf,
    declared_namespace: String,
}
struct Binding {
    scope: u32,
    target: Option<u32>,
    module: Option<String>,
    imported: Option<String>,
    type_name: Option<String>,
    kind: String,
    position: usize,
}

pub(crate) fn run(
    connection: &rusqlite::Connection,
    cancel: &Cancellation,
    selection: super::incremental::Resolution,
) -> Result<()> {
    let connection = &Connection::new(connection)?;
    let all = selection == super::incremental::Resolution::All;
    if all {
        crate::infrastructure::index::relations::clear_all(connection)?;
        connection.cached_execute(
            "DELETE FROM facts WHERE relation IN ('overrides','dispatch_candidate')
             OR (language='go' AND relation='implements' AND expression LIKE '@go-interface:%')",
            [],
        )?;
        connection.execute_batch("DELETE FROM lookup_blocks;")?;
    } else {
        crate::infrastructure::index::invalidation::prepare_partial(connection)?;
    }
    connection.execute_batch(include_str!("staging.sql"))?;
    crate::application::analysis::resolve::modules::scopes::reset(connection)?;
    ejs::diagnostics(connection)?;
    connection.execute_batch(include_str!("path_caches.sql"))?;
    publication::reset(connection)?;
    crate::application::analysis::resolve::python::mro::reset(connection)?;
    crate::application::analysis::resolve::php::members::reset(connection)?;
    let prepared = prepared::Prepared::new(connection.connection())?;
    preparation::run(connection, cancel, &prepared, all)?;
    connection.execute_batch(include_str!("lisp/packages.sql"))?;
    let merged: bool =
        connection.cached_row("SELECT EXISTS(SELECT 1 FROM symbol_aliases)", [], |r| {
            r.get(0)
        })?;

    facts::run(connection, cancel, &prepared, merged, !all)?;

    publication::flush(connection)?;

    if all {
        crate::application::analysis::resolve::go::interfaces::run(connection, cancel)?;

        dispatch::run(connection, cancel, false)?;

        crate::application::analysis::resolve::go::interfaces::dispatch(connection, cancel, false)?;
    } else {
        dispatch::run(connection, cancel, true)?;
        crate::application::analysis::resolve::go::interfaces::refresh_calls(connection, cancel)?;
    }
    connection.execute_batch("DELETE FROM relation_texts WHERE NOT EXISTS(SELECT 1 FROM relation_routes r WHERE r.kind=6 AND r.entity=relation_texts.id);")?;

    Ok(())
}

fn context(
    connection: &Connection,
    file: i64,
    language: Option<Language>,
) -> Result<Option<FileContext>> {
    let record: (String, String, String, Vec<u8>, String) = connection.cached_row(
        "SELECT language,module,package,path,declared_module FROM files WHERE id=?1",
        [file],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    let input_format = record.0.parse().ok();
    let Some(language) = language.or(input_format) else {
        return Ok(None);
    };
    Ok(Some(FileContext {
        id: file,
        language,
        input_format,
        module: record.1,
        package: record.2,
        path: path_from_bytes(&record.3)?,
        declared_namespace: record.4,
    }))
}

struct Lookup<'conn, 'operation> {
    connection: &'operation Connection<'conn>,
    fact: Option<i64>,
    relation: &'operation str,
    external_scope_id: usize,
    scope_sequence: usize,
    external_present: bool,
    reason: Option<&'static str>,
    rule: &'static str,
    source_override: Option<i64>,
    member_depth: usize,
    candidate_only: bool,
    language: Language,
    namespace: crate::domain::resolve::namespaces::Namespace,
    receiver: bool,
    ruby_side_checked: bool,
    ruby_side: Option<ruby::Side>,
    prepared: &'operation prepared::Prepared<'conn>,
}

impl Lookup<'_, '_> {
    fn dependency(&self, kind: &str, key: &str, found: bool) -> Result<()> {
        let Some(fact) = self.fact else { return Ok(()) };
        self.prepared.dependency(params![fact, kind, key, found])?;
        Ok(())
    }
}
