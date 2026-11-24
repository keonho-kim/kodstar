// Purpose: 파일·객체 선택과 정의 행의 순차 조회.
// Pattern: Simple Module.
// Usage: 조회 조정과 파일 연결 조회에서 호출.
// Related: crates/kodstar-core/src/application/query/mod.rs, crates/kodstar-core/src/application/query/relationships.rs.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::query::{Query, Selector};
use crate::domain::results::{QueryRow, QuerySink, QuerySummary};
use crate::infrastructure::cache::paths::{path_bytes, relative_source};
use crate::infrastructure::index::{self, SYMBOL_COLUMNS};

use crate::application::workspace::SelectedProject;

use super::reached;

pub(super) fn requested_files(
    connection: &Connection,
    selected: &SelectedProject,
    query: &Query,
) -> (Option<Result<i64>>, Option<Result<i64>>) {
    let (first, second): (Option<&Path>, Option<&Path>) = match query {
        Query::Symbols { file } | Query::Files { file } | Query::Impact { file, .. } => {
            (Some(file), None)
        }
        Query::Related { selector }
        | Query::Deps { selector }
        | Query::Refs { selector }
        | Query::Explain { selector } => (Some(&selector.file), None),
        Query::Path { from, to } => (Some(&from.file), Some(&to.file)),
        _ => (None, None),
    };
    let identify = |file| file_id(connection, selected, file);
    (first.map(identify), second.map(identify))
}

pub(super) fn prepared_file(file: Option<Result<i64>>) -> Result<i64> {
    file.ok_or_else(|| Error::operation("missing prepared query file"))?
}

fn file_id(connection: &Connection, selected: &SelectedProject, file: &Path) -> Result<i64> {
    let path = if selected.historical {
        crate::infrastructure::history::paths::from_input(&selected.root, &selected.cwd, file)?
    } else {
        relative_source(&selected.root, &selected.cwd, file)?
    };
    connection
        .query_row(
            "SELECT id FROM files WHERE path=?1",
            [path_bytes(&path)],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::not_found(format!("file is not indexed: {}", path.display())))
}

pub(super) fn select(
    connection: &Connection,
    file: i64,
    selector: &Selector,
    limit: Option<usize>,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    summary: &mut QuerySummary,
) -> Result<i64> {
    let (line, column) = selector
        .at
        .map_or((None, None), |(line, column)| (Some(line), Some(column)));
    let lisp_key = crate::domain::resolve::lisp::key(&selector.name);
    let rust_key = crate::domain::languages::Language::Rust.lookup_key(&selector.name);
    let predicate = "s.file_id=?1 AND (s.name=?2 OR s.qualified_name=?2 OR (s.language='commonlisp' AND s.name_key=?5) OR (s.language='rust' AND (s.name_key=?6 OR replace(s.qualified_name,'r#','')=?6))) AND (?3 IS NULL OR (s.line=?3 AND s.column_byte=?4))";
    let count: i64 = connection.query_row(
        &format!("SELECT COUNT(DISTINCT COALESCE(g.canonical_id,s.id)) FROM symbols s JOIN files f ON f.id=s.file_id LEFT JOIN symbol_aliases g ON g.symbol_id=s.id WHERE {predicate}"),
        params![file, selector.name, line, column,lisp_key,rust_key],
        |row| row.get(0),
    )?;
    if count == 0 {
        if index::failures::unavailable(connection, file)? {
            let mut error = Error::input(
                "source_unavailable",
                format!(
                    "object selection is unavailable because analysis of {} is incomplete; run kodstar doctor for details",
                    selector.file.display()
                ),
                None,
            );
            error.kind = crate::domain::diagnostics::ErrorKind::Limit;
            return Err(error);
        }
        return Err(Error::not_found(format!(
            "object not found: {}::{}",
            selector.file.display(),
            selector.name
        )));
    }
    if count > 1 {
        let mut statement=connection.prepare(&format!("SELECT {SYMBOL_COLUMNS} FROM symbols s JOIN files f ON f.id=s.file_id WHERE {predicate} ORDER BY s.line,s.column_byte,s.qualified_name"))?;
        for row in statement.query_map(
            params![file, selector.name, line, column, lisp_key, rust_key],
            |row| Ok((index::symbol_from_row(row)?, row.get::<_, u64>(20)?)),
        )? {
            cancel.check()?;
            if reached(limit, summary) {
                break;
            }
            let (mut symbol, definitions) = row?;
            index::complete_symbol(connection, &mut symbol, definitions)?;
            sink.row(&QueryRow::Symbol(symbol))?;
            summary.result_count += 1;
        }
        return Err(Error::not_found(
            "object selection is ambiguous; select a qualified name or --at line:column",
        ));
    }
    Ok(connection.query_row(
        &format!("SELECT COALESCE(g.canonical_id,s.id) FROM symbols s JOIN files f ON f.id=s.file_id LEFT JOIN symbol_aliases g ON g.symbol_id=s.id WHERE {predicate} LIMIT 1"),
        params![file, selector.name, line, column,lisp_key,rust_key],
        |row| row.get(0),
    )?)
}

pub(super) fn stream_symbols(
    connection: &Connection,
    predicate: &str,
    parameters: &[&dyn rusqlite::ToSql],
    limit: Option<usize>,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    summary: &mut QuerySummary,
) -> Result<()> {
    let aliased: bool =
        connection.query_row("SELECT EXISTS(SELECT 1 FROM symbol_aliases)", [], |r| {
            r.get(0)
        })?;
    let predicate = if aliased {
        format!("s.id IN (
        SELECT id FROM (SELECT s.id,ROW_NUMBER() OVER(PARTITION BY COALESCE(g.canonical_id,s.id) ORDER BY f.path,s.start_byte,s.syntax_blob) AS rank
          FROM symbols s JOIN files f ON f.id=s.file_id LEFT JOIN symbol_aliases g ON g.symbol_id=s.id WHERE ({predicate})) WHERE rank=1
        )")
    } else {
        predicate.to_owned()
    };
    let mut statement = connection.prepare(&format!(
        "SELECT {SYMBOL_COLUMNS} FROM symbols s JOIN files f ON f.id=s.file_id WHERE ({predicate})
        ORDER BY f.path,s.line,s.column_byte,s.qualified_name,s.kind LIMIT {}",
        super::sql_limit(limit)
    ))?;
    let mut rows = statement.query(parameters)?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if reached(limit, summary) {
            break;
        }
        let mut symbol = index::symbol_from_row(row)?;
        index::complete_symbol(connection, &mut symbol, row.get(20)?)?;
        sink.row(&QueryRow::Symbol(symbol))?;
        summary.result_count += 1;
    }
    Ok(())
}
