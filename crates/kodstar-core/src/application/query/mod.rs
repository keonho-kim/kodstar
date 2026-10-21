// Purpose: 조회 종류·metadata·결과 한도와 완료 조정.
// Pattern: Coordinator.
// Usage: Engine이 읽기 snapshot과 sink를 전달.
// Related: crates/kodstar-core/src/application/engine.rs, crates/kodstar-core/src/application/query/selection.rs.

//! Query orchestration on one committed generation.
mod relationship_rows;
mod relationships;
mod search;
mod selection;
mod traversal;

use rusqlite::Connection;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::query::{Query, QueryOptions, Selector};
use crate::domain::relations::Direction;
use crate::domain::results::{QueryMetadata, QueryRow, QuerySink, QuerySummary};
use crate::infrastructure::cache::paths::encoding;
use crate::infrastructure::index::{self};

use crate::application::workspace::SelectedProject;

use relationships::{files, stream_relationships};
use selection::{file_id, select, stream_symbols};
use traversal::{members, restrict_to_path, setup, traverse};

pub(crate) fn run(
    connection: &Connection,
    selected: &SelectedProject,
    query: &Query,
    options: &QueryOptions,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    maintenance: Option<crate::domain::results::MaintenanceReport>,
) -> Result<QuerySummary> {
    options.validate(query)?;
    let (generation, indexed_at,relation_registry_version): (i64, u64,u32) = connection.query_row(
        "SELECT generation,indexed_at,relation_registry_version FROM metadata WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?,row.get(2)?)),
    )?;
    if generation == 0 {
        return Err(Error::not_found(
            "no completed index found; run kodstar build first",
        ));
    }
    let coverage = index::coverage(connection)?;
    let metadata = QueryMetadata {
        maintenance,
        schema_version: 1,
        relation_registry_version,
        engine_version: crate::VERSION,
        root: Some(selected.root.to_string_lossy().into_owned()),
        root_encoding: encoding(&selected.root),
        generation: Some(generation),
        indexed_at: Some(indexed_at),
        freshness: options.refresh.as_str(),
        unresolved_count: coverage.unresolved,
        coverage,
    };
    sink.begin(&metadata)?;
    let mut summary = QuerySummary {
        complete: true,
        unresolved_count: metadata.unresolved_count,
        ..QuerySummary::default()
    };
    let limit = options.limit.resolve(selected.settings.limit);
    let outcome: Result<()> = (|| {
        match query {
            Query::Symbols { file } => {
                let file = file_id(connection, selected, file)?;
                stream_symbols(
                    connection,
                    "s.file_id=?1 AND s.kind<>'file'",
                    &[&file],
                    limit,
                    cancel,
                    sink,
                    &mut summary,
                )?;
            }
            Query::Search { name, prefix } => {
                search::run(connection, name, *prefix, limit, cancel, sink, &mut summary)?
            }
            Query::Files { file } => {
                let file = file_id(connection, selected, file)?;
                files(connection, file, options, limit, cancel, sink, &mut summary)?;
            }
            _ => {
                setup(connection, options, matches!(query, Query::Explain { .. }))?;
                let max_nodes = options.max_nodes.resolve(selected.settings.max_nodes);
                match query {
                    Query::Related { selector }
                    | Query::Deps { selector }
                    | Query::Refs { selector }
                    | Query::Explain { selector } => {
                        let id = select(
                            connection,
                            selected,
                            selector,
                            limit,
                            cancel,
                            sink,
                            &mut summary,
                        )?;
                        connection.execute("INSERT INTO q_seeds VALUES(?1)", [id])?;
                        if options.members {
                            members(connection)?;
                        }
                        if matches!(query, Query::Explain { .. }) {
                            sink.row(&QueryRow::Symbol(index::symbol(connection, id)?))?;
                            summary.result_count += 1;
                            if options.direction != Some(Direction::In) {
                                connection.execute("INSERT OR IGNORE INTO q_edges SELECT r.id,'out',1 FROM q_allowed r WHERE r.source_id IN (SELECT id FROM q_seeds)",[])?;
                            }
                            if options.direction != Some(Direction::Out) {
                                connection.execute("INSERT OR IGNORE INTO q_edges SELECT r.id,'in',1 FROM q_allowed r WHERE r.target_id IN (SELECT id FROM q_seeds)",[])?;
                            }
                        } else {
                            traverse(
                                connection,
                                options.direction_for(query),
                                options.depth_for(query),
                                max_nodes,
                                cancel,
                                &mut summary,
                                None,
                            )?;
                        }
                    }
                    Query::Impact { file, name, at } => {
                        let file_key = file_id(connection, selected, file)?;
                        if let Some(name) = name {
                            let selector = Selector {
                                file: file.clone(),
                                name: name.clone(),
                                at: *at,
                            };
                            let id = select(
                                connection,
                                selected,
                                &selector,
                                limit,
                                cancel,
                                sink,
                                &mut summary,
                            )?;
                            connection.execute("INSERT INTO q_seeds VALUES(?1)", [id])?;
                            members(connection)?;
                        } else {
                            connection.execute(
                                "INSERT INTO q_seeds SELECT id FROM symbols WHERE file_id=?1",
                                [file_key],
                            )?;
                        }
                        traverse(
                            connection,
                            Direction::In,
                            options.depth_for(query),
                            max_nodes,
                            cancel,
                            &mut summary,
                            None,
                        )?;
                    }
                    Query::Path { from, to } => {
                        let from = select(
                            connection,
                            selected,
                            from,
                            limit,
                            cancel,
                            sink,
                            &mut summary,
                        )?;
                        let to =
                            select(connection, selected, to, limit, cancel, sink, &mut summary)?;
                        connection.execute("INSERT INTO q_seeds VALUES(?1)", [from])?;
                        if from == to {
                            sink.row(&QueryRow::Symbol(index::symbol(connection, from)?))?;
                            summary.result_count += 1;
                            return Ok(());
                        }
                        traverse(
                            connection,
                            options.direction_for(query),
                            options.depth_for(query),
                            max_nodes,
                            cancel,
                            &mut summary,
                            Some(to),
                        )?;
                        restrict_to_path(connection, to)?;
                    }
                    _ => {}
                }
                stream_relationships(
                    connection,
                    options.sites || matches!(query, Query::Explain { .. }),
                    limit,
                    cancel,
                    sink,
                    &mut summary,
                )?;
            }
        }
        Ok(())
    })();
    if let Err(error) = &outcome {
        summary.complete = false;
        summary.reasons.push(match error.kind {
            crate::domain::diagnostics::ErrorKind::NotFound => "selection_error",
            crate::domain::diagnostics::ErrorKind::Limit => "resource_limit",
            _ => "query_error",
        });
    }
    sink.finish(&summary)?;
    outcome?;
    Ok(summary)
}

fn reached(limit: Option<usize>, summary: &mut QuerySummary) -> bool {
    if limit.is_some_and(|limit| summary.result_count >= limit as u64) {
        summary.truncated = true;
        summary.complete = false;
        if !summary.reasons.contains(&"result_limit") {
            summary.reasons.push("result_limit");
        }
        true
    } else {
        false
    }
}

fn sql_limit(limit: Option<usize>) -> i64 {
    limit
        .and_then(|n| n.checked_add(1))
        .and_then(|n| i64::try_from(n).ok())
        .unwrap_or(-1)
}
