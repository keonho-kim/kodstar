// Purpose: 조회 종류·metadata·결과 한도와 완료 조정.
// Pattern: Coordinator.
// Usage: Engine이 읽기 snapshot과 sink를 전달.
// Related: crates/kodstar-core/src/application/engine.rs, crates/kodstar-core/src/application/query/selection.rs.

//! Query orchestration on one committed generation.
mod documentation;
pub(crate) mod metadata;
mod overview;
mod ranked;
mod relationship_rows;
mod relationships;
mod search;
mod selection;
mod traversal;

use rusqlite::Connection;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::query::{Query, QueryOptions, Selector};
use crate::domain::relations::Direction;
use crate::domain::results::{MaintenanceReport, QueryRow, QuerySink, QuerySummary};
use crate::infrastructure::history::Repository;
use crate::infrastructure::index::{self};

use crate::application::workspace::SelectedProject;

use relationships::{files, stream_relationships};
use selection::{prepared_file, select, stream_symbols};
use traversal::{members, restrict_to_path, setup, traverse};

pub(crate) struct ReadScope<'a> {
    pub selected: &'a SelectedProject,
    pub maintenance: Option<MaintenanceReport>,
    pub repository: Option<&'a mut Repository>,
}

pub(crate) fn run(
    connection: &Connection,
    scope: ReadScope<'_>,
    query: &Query,
    options: &QueryOptions,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<QuerySummary> {
    let span = tracing::info_span!(
        "query",
        rows = tracing::field::Empty,
        visited = tracing::field::Empty
    );
    let _entered = span.enter();
    options.validate(query)?;
    let selected = scope.selected;
    let (first, second) = selection::requested_files(connection, selected, query);
    // Reading metadata pins the WAL snapshot before publication by another command.
    let metadata = metadata::read(connection, selected, options.refresh, scope.maintenance)?;
    if let Some(repository) = scope.repository {
        repository.release();
    }
    sink.begin(&metadata)?;
    let mut summary = QuerySummary {
        complete: true,
        unresolved_count: metadata.unresolved_count,
        ..QuerySummary::default()
    };
    summary.include_coverage(&metadata.coverage);
    let limit = options.limit.resolve(selected.settings.limit);
    let outcome: Result<()> = (|| {
        match query {
            Query::Symbols { .. } => {
                let file = prepared_file(first)?;
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
            Query::RankedSearch { text } => {
                ranked::run(
                    connection,
                    text,
                    &selected.settings,
                    options,
                    cancel,
                    sink,
                    &mut summary,
                )?;
            }
            Query::Overview => {
                overview::run(
                    connection,
                    &selected.settings,
                    options,
                    cancel,
                    sink,
                    &mut summary,
                )?;
            }
            Query::Files { .. } => {
                let file = prepared_file(first)?;
                files(connection, file, options, limit, cancel, sink, &mut summary)?;
            }
            _ => {
                setup(connection, options, matches!(query, Query::Explain { .. }))?;
                let limits = || traversal::Limits {
                    maximum: options.max_nodes.resolve(selected.settings.max_nodes),
                    memory_bytes: (selected.settings.memory_budget / 64).min(512 * 1024),
                };
                match query {
                    Query::Changes { .. } => {
                        connection.execute("INSERT INTO q_seeds SELECT s.id FROM symbols s JOIN files f ON f.id=s.file_id JOIN q_changed_inputs c ON c.path=f.path",[])?;
                        traverse(
                            connection,
                            Direction::In,
                            options.depth_for(query),
                            limits(),
                            cancel,
                            &mut summary,
                            None,
                        )?;
                    }
                    Query::Related { selector }
                    | Query::Deps { selector }
                    | Query::Refs { selector }
                    | Query::Explain { selector } => {
                        let id = select(
                            connection,
                            prepared_file(first)?,
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
                                limits(),
                                cancel,
                                &mut summary,
                                None,
                            )?;
                        }
                    }
                    Query::Impact { file, name, at } => {
                        let file_key = prepared_file(first)?;
                        if let Some(name) = name {
                            let selector = Selector {
                                file: file.clone(),
                                name: name.clone(),
                                at: *at,
                            };
                            let id = select(
                                connection,
                                file_key,
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
                            limits(),
                            cancel,
                            &mut summary,
                            None,
                        )?;
                    }
                    Query::Path { from, to } => {
                        let from = select(
                            connection,
                            prepared_file(first)?,
                            from,
                            limit,
                            cancel,
                            sink,
                            &mut summary,
                        )?;
                        let to = select(
                            connection,
                            prepared_file(second)?,
                            to,
                            limit,
                            cancel,
                            sink,
                            &mut summary,
                        )?;
                        connection.execute("INSERT INTO q_seeds VALUES(?1)", [from])?;
                        if from == to {
                            sink.row(&QueryRow::Symbol(index::symbol(connection, from)?))?;
                            summary.result_count += 1;
                            return Ok(());
                        }
                        let direction = options.direction_for(query);
                        traverse(
                            connection,
                            direction,
                            options.depth_for(query),
                            limits(),
                            cancel,
                            &mut summary,
                            Some(to),
                        )?;
                        restrict_to_path(connection, to, direction)?;
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
                if matches!(query, Query::Explain { .. }) {
                    documentation::emit(connection, selected, limit, cancel, sink, &mut summary)?;
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = &outcome {
        summary.complete = false;
        summary.reasons.push(
            if error
                .input
                .as_ref()
                .is_some_and(|i| i.code == "source_unavailable")
            {
                "source_unavailable"
            } else {
                match error.kind {
                    crate::domain::diagnostics::ErrorKind::NotFound => "selection_error",
                    crate::domain::diagnostics::ErrorKind::Limit => "resource_limit",
                    _ => "query_error",
                }
            },
        );
    }
    span.record("rows", summary.result_count);
    span.record("visited", summary.visited_count);
    sink.finish(&summary)?;
    outcome?;
    Ok(summary)
}

fn reached(limit: Option<usize>, summary: &mut QuerySummary) -> bool {
    if limit.is_some_and(|limit| summary.result_count >= limit as u64) {
        summary.truncate("result_limit");
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
