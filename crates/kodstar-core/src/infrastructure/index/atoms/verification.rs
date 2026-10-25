// Purpose: 실제 모든 물리 문자열 참조와 저장 계수의 일치 확인.
// Pattern: Integrity Check.
// Usage: doctor가 같은 읽기 transaction에서 실행.
// Related: crates/kodstar-core/src/infrastructure/index/records/mod.rs, crates/kodstar-core/src/infrastructure/index/atoms/mod.rs.

use crate::domain::diagnostics::{Error, Result};
use rusqlite::Connection;

pub(crate) fn verify(c: &Connection) -> Result<()> {
    let sources = super::super::records::LAYOUTS
        .iter()
        .flat_map(|l| {
            l.atoms.iter().map(move |k| {
                format!(
                    "SELECT {k} AS id FROM {}_rows WHERE {k} IS NOT NULL",
                    l.name
                )
            })
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    c.execute_batch(&format!(
        "CREATE TEMP TABLE atom_expected(id INTEGER PRIMARY KEY,refs INTEGER);
        INSERT INTO atom_expected SELECT id,count(*) FROM ({sources}) GROUP BY id;"
    ))?;
    let invalid: bool = c.query_row(
        "SELECT EXISTS(SELECT id,refs FROM atom_expected EXCEPT SELECT id,refs FROM atoms)
        OR EXISTS(SELECT id,refs FROM atoms EXCEPT SELECT id,refs FROM atom_expected)",
        [],
        |r| r.get(0),
    )?;
    c.execute_batch("DROP TABLE atom_expected;")?;
    if invalid {
        return Err(Error::operation(
            "analysis string reference counts do not match stored records",
        ));
    }
    Ok(())
}
