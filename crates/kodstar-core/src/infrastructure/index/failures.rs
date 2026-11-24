// Purpose: 실패 파일의 불완전한 사실 제거와 구조화된 입력 진단 저장.
// Pattern: Storage Adapter.
// Usage: analysis writer가 정상 세대와 같은 transaction에서 호출.
// Related: crates/kodstar-core/src/infrastructure/index/write.rs, crates/kodstar-core/src/application/analysis/pipeline/writer.rs.

use super::FileRecord;
use crate::domain::{
    diagnostics::{InputIssue, Result},
    native::Report,
};
use rusqlite::{Connection, params};

pub(crate) fn replace(
    connection: &super::cached::Sql<'_>,
    mut record: FileRecord,
    issue: InputIssue,
    message: &str,
    native: Option<&Report>,
) -> Result<()> {
    connection.execute(
        "INSERT OR IGNORE INTO seen_files VALUES(?1)",
        [crate::infrastructure::cache::paths::path_bytes(
            &record.path,
        )],
    )?;
    record.status = "skipped";
    let id = record.persist(connection)?;
    super::clear_file(connection, id)?;
    super::native::replace(connection, id, native)?;
    connection.execute(
        "INSERT INTO diagnostics(file_id,code,message,line) VALUES(?1,?2,?3,?4)",
        params![id, issue.code, message, issue.line],
    )?;
    Ok(())
}

pub(crate) fn count(connection: &Connection) -> Result<u64> {
    Ok(connection.query_row("SELECT COUNT(DISTINCT file_id) FROM diagnostics WHERE code IN
        ('source_syntax_error','source_encoding_error','source_analysis_limit','unsupported_source_construct','configuration_invalid')",
        [], |row| row.get(0))?)
}

pub(crate) fn unavailable(connection: &Connection, file: i64) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM files WHERE id=?1 AND status='skipped')
        OR EXISTS(SELECT 1 FROM diagnostics WHERE file_id=?1 AND code IN ('source_syntax_partial','source_macro_incomplete','unexpanded_native_macros'))",
        [file],
        |r| r.get(0),
    )?)
}

pub(crate) fn was_partial(connection: &Connection, path: &std::path::Path) -> Result<bool> {
    Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM diagnostics d JOIN files f ON f.id=d.file_id WHERE f.path=?1 AND d.code IN ('source_syntax_partial','source_macro_incomplete'))", [crate::infrastructure::cache::paths::path_bytes(path)], |r| r.get(0))?)
}

pub(crate) fn partial(
    connection: &Connection,
    file: i64,
    span: Option<crate::domain::source::SourceSpan>,
) -> Result<()> {
    connection.execute(
        "DELETE FROM diagnostics WHERE file_id=?1 AND code='source_syntax_partial'",
        [file],
    )?;
    if let Some(span) = span {
        connection.execute("INSERT INTO diagnostics(file_id,code,message,line) VALUES(?1,'source_syntax_partial',?2,?3)", params![file, format!("Tree-sitter recovered source at {}:{}; uncertain scopes and providers retain candidate relationships", span.line, span.column), span.line])?;
    }
    Ok(())
}

pub(crate) fn fragments(connection: &Connection, file: i64, lines: &[usize]) -> Result<()> {
    connection.execute(
        "DELETE FROM diagnostics WHERE file_id=?1 AND code='source_fragment_context'",
        [file],
    )?;
    for line in lines {
        connection.execute("INSERT INTO diagnostics(file_id,code,message,line) VALUES(?1,'source_fragment_context',?2,?3)",
            params![file, "Macro-provided syntax is a token fragment; complete expression semantics are unavailable", line])?;
    }
    Ok(())
}
