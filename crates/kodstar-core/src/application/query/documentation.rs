// Purpose: explain에서 구문 주석을 같은 체크포인트의 문서 위치에 연결.
// Pattern: Query.
// Usage: 조회 조정자가 선택한 객체와 읽기 세대의 snapshot을 전달.
// Related: crates/kodstar-core/src/application/query/mod.rs, crates/kodstar-core/src/infrastructure/history/inputs/mod.rs.

mod markdown;

use std::{path::Path, rc::Rc};

use rusqlite::Connection;

use crate::application::workspace::SelectedProject;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, Result},
    resolve::documentation::{PREFIX, reference},
    results::{DocumentationReference, QueryRow, QuerySink, QuerySummary, UseSite},
    source::SourceSpan,
};
use crate::infrastructure::{
    cache::paths::{encoding, path_from_bytes},
    history::{inputs, paths},
};

pub(super) fn emit(
    c: &Connection,
    selected: &SelectedProject,
    limit: Option<usize>,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    summary: &mut QuerySummary,
) -> Result<()> {
    let mut statement = c.prepare(
        "WITH seeds AS (
           SELECT s.id,s.file_id,s.owner_id,s.line,s.start_byte
           FROM q_seeds q JOIN symbol_identities a ON q.id=a.canonical_id
           JOIN symbols_rows s ON s.id=a.symbol_id
         ), uses(id) AS (
           SELECT f.id FROM seeds s CROSS JOIN facts f ON f.source_id=s.id
           WHERE f.relation='references' AND substr(f.expression,1,length(?1))=?1
           UNION
           SELECT f.id FROM seeds s CROSS JOIN facts f ON f.source_id=s.owner_id AND f.file_id=s.file_id
           WHERE f.relation='references' AND substr(f.expression,1,length(?1 || 'leading:'))=?1 || 'leading:'
             AND f.end_line IN (s.line-1,s.line) AND f.end_byte<=s.start_byte
         )
         SELECT file.path,f.expression,f.start_byte,f.end_byte,f.line,f.column_byte,f.end_line,f.end_column_byte
         FROM uses u JOIN facts f ON f.id=u.id JOIN files_rows file ON file.id=f.file_id
         ORDER BY file.path,f.start_byte,f.expression,f.id",
    )?;
    let mut rows = statement.query([PREFIX])?;
    let Some(mut row) = rows.next()? else {
        return Ok(());
    };
    if super::reached(limit, summary) {
        return Ok(());
    }
    let snapshot = selected
        .snapshot
        .as_ref()
        .ok_or_else(|| Error::operation("explanation has no source checkpoint"))?;
    let connection = Rc::new(snapshot.open(cancel)?);
    let mut reader = inputs::Reader::new(&connection)?;
    let mut budget = (selected.settings.memory_budget / 16).min(1024 * 1024);
    loop {
        cancel.check()?;
        let path = path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        let expression: String = row.get(1)?;
        let (kind, target) = reference(&expression)
            .ok_or_else(|| Error::operation("invalid stored documentation reference"))?;
        let source = location(
            &path,
            SourceSpan {
                start_byte: row.get(2)?,
                end_byte: row.get(3)?,
                line: row.get(4)?,
                column: row.get(5)?,
                end_line: row.get(6)?,
                end_column: row.get(7)?,
            },
        );
        let (destination, reason) =
            match find(&mut reader, selected, &path, target, &mut budget, cancel)? {
                Ok(target) => (Some(target), None),
                Err(reason) => (None, Some(reason)),
            };
        if reason == Some("document_byte_limit") {
            summary.truncate("document_byte_limit");
        }
        sink.row(&QueryRow::Documentation(Box::new(DocumentationReference {
            kind: "documentation_reference",
            document_kind: kind,
            reference: target.into(),
            source,
            target: destination,
            resolution: if reason.is_some() {
                "unresolved"
            } else {
                "resolved"
            },
            reason,
        })))?;
        summary.result_count += 1;
        let Some(next) = rows.next()? else {
            break;
        };
        if super::reached(limit, summary) {
            break;
        }
        row = next;
    }
    Ok(())
}

fn find(
    reader: &mut inputs::Reader<'_>,
    project: &SelectedProject,
    source: &Path,
    reference: &str,
    budget: &mut usize,
    cancel: &Cancellation,
) -> Result<std::result::Result<UseSite, &'static str>> {
    let (path, anchor) = reference
        .split_once('#')
        .map_or((reference, None), |(path, anchor)| (path, Some(anchor)));
    if Path::new(path).is_absolute() || path.contains([':', '\\']) {
        return Ok(Err("outside_project"));
    }
    let base = if path.starts_with("./") || path.starts_with("../") {
        project
            .root
            .join(source.parent().unwrap_or_else(|| Path::new("")))
    } else {
        project.root.clone()
    };
    let path = match paths::from_input(&project.root, &base, Path::new(path)) {
        Ok(path) => path,
        Err(_) => return Ok(Err("outside_project")),
    };
    let document = match reader.get(&path) {
        Ok(entry) => entry,
        Err(error) if error.kind == ErrorKind::NotFound => {
            return Ok(Err("document_not_in_checkpoint"));
        }
        Err(error) => return Err(error),
    };
    let maximum = (*budget)
        .min(256 * 1024)
        .min(project.settings.max_file_bytes);
    if document.size() > maximum as u64 {
        return Ok(Err("document_byte_limit"));
    }
    let bytes = document.read(maximum, cancel)?;
    *budget -= bytes.len();
    let Ok(value) = std::str::from_utf8(&bytes) else {
        return Ok(Err("document_not_utf8"));
    };
    Ok(markdown::heading(value, anchor).map(|span| location(&path, span)))
}

fn location(path: &Path, span: SourceSpan) -> UseSite {
    UseSite {
        file: path.to_string_lossy().into_owned(),
        path_encoding: encoding(path),
        span,
    }
}
