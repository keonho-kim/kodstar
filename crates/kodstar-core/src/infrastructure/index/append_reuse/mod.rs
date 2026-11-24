// Purpose: 증명된 선언 삽입에서 기존 사실·관계를 보존하고 새 해석 범위만 공개.
// Pattern: Transactional Delta.
// Usage: writer가 기존 파일을 지우기 전에 후보와 실제 이름 소비자를 확인.
// Related: crates/kodstar-core/src/domain/facts/append.rs, crates/kodstar-core/src/application/analysis/pipeline/start.rs.

use super::{FileFacts, FileRecord, cached::Sql};
use crate::domain::{diagnostics::Result, facts::append::Candidate};
use std::ops::Range;

#[derive(Clone)]
pub(crate) struct RetainedRows {
    pub range: Range<usize>,
    pub facts: i64,
    pub bindings: i64,
}

pub(crate) fn begin(c: &rusqlite::Connection) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS append_symbols(id INTEGER PRIMARY KEY); DELETE FROM append_symbols;
        CREATE TEMP VIEW IF NOT EXISTS symbol_additions AS SELECT s.* FROM symbols s JOIN append_symbols a ON a.id=s.id;")?;
    Ok(())
}

pub(crate) fn start(
    c: &Sql<'_>,
    record: &FileRecord,
    candidate: &Candidate,
) -> Result<Option<FileFacts>> {
    if matches!(record.language.as_str(), "c" | "cpp") {
        return Ok(None);
    }
    let Some(id) = super::invalidation::prior_file(c, record)? else {
        return Ok(None);
    };
    for name in &candidate.names {
        // This also rejects existing negative/ambiguous lookups, aliases, signatures,
        // re-exports and scope names. A fresh spelling has no prior consumer to rebind.
        let mentioned: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM atoms WHERE instr(lower(value),lower(?1))>0)
             OR EXISTS(SELECT 1 FROM lookup_keys WHERE instr(lower(lookup_key),lower(?1))>0)",
            [name],
            |r| r.get(0),
        )?;
        if mentioned {
            return Ok(None);
        }
    }
    record.refresh_metadata(c)?;
    super::invalidation::capture_imports(c, record)?;
    c.execute("INSERT OR IGNORE INTO changed_files VALUES(?1)", [id])?;
    c.execute(
        "INSERT OR IGNORE INTO body_files(id,granular) VALUES(?1,1)",
        [id],
    )?;
    let mut context = FileFacts::new(id, record, false);
    let facts = c.query_row(
        "SELECT COALESCE(MAX(id),0) FROM facts_rows WHERE file_id=?1",
        [id],
        |r| r.get(0),
    )?;
    let bindings = c.query_row(
        "SELECT COALESCE(MAX(id),0) FROM bindings_rows WHERE file_id=?1",
        [id],
        |r| r.get(0),
    )?;
    context.addition = Some(RetainedRows {
        range: candidate.range.clone(),
        facts,
        bindings,
    });
    Ok(Some(context))
}
