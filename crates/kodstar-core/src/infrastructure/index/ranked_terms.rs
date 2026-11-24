// Purpose: 바뀐 이름·경로의 순위 검색 단어를 같은 분석 세대에 반영.
// Pattern: Derived Index.
// Usage: 분석 transaction이 정의와 별칭을 확정한 뒤 호출.
// Related: crates/kodstar-core/src/infrastructure/index/ranked_terms.sql, crates/kodstar-core/src/domain/query/terms.rs.

use crate::domain::{cancellation::Cancellation, diagnostics::Result, query::terms::words};
use rusqlite::{Connection, params};

pub(super) const SCHEMA: &str = include_str!("ranked_terms.sql");

pub(crate) fn refresh(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    connection.execute_batch(
        "DELETE FROM ranked_symbol_terms WHERE id IN (SELECT id FROM ranked_symbols_pending);
         DELETE FROM ranked_file_terms WHERE id IN (SELECT id FROM ranked_files_pending);",
    )?;
    for (select, insert) in [
        (
            "SELECT s.id,s.name,s.qualified_name FROM ranked_symbols_pending p
             CROSS JOIN symbols s ON s.id=p.id WHERE s.kind<>'file'",
            "INSERT OR IGNORE INTO ranked_symbol_terms VALUES(?1,?2)",
        ),
        (
            "SELECT f.id,f.display_path,NULL FROM ranked_files_pending p
             CROSS JOIN files f ON f.id=p.id",
            "INSERT OR IGNORE INTO ranked_file_terms VALUES(?1,?2)",
        ),
    ] {
        let mut read = connection.prepare(select)?;
        let mut write = connection.prepare(insert)?;
        let mut rows = read.query([])?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let qualified: Option<String> = row.get(2)?;
            let mut terms = words(&name);
            if let Some(qualified) = qualified.filter(|qualified| qualified != &name) {
                terms.extend(words(&qualified));
            }
            terms.sort_unstable();
            terms.dedup();
            for term in terms {
                write.execute(params![term, id])?;
            }
        }
    }
    connection
        .execute_batch("DELETE FROM ranked_symbols_pending; DELETE FROM ranked_files_pending;")?;
    Ok(())
}
