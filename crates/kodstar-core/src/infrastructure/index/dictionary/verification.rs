// Purpose: 사전 payload·정렬 경계·안정 ID 검색 경로의 무결성 확인.
// Pattern: Integrity Check.
// Usage: doctor가 같은 분석 snapshot에서만 호출.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/mod.rs.

use crate::domain::diagnostics::{Error, Result};
use rusqlite::{Connection, params};

pub(crate) fn verify(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TEMP TABLE dictionary_expected(id INTEGER PRIMARY KEY,block INTEGER); CREATE TEMP TABLE dictionary_expected_large(kind TEXT,key TEXT,PRIMARY KEY(kind,key)) WITHOUT ROWID;",
    )?;
    let mut select = c.prepare(
        "SELECT id,kind,first,last,count,payload,digest FROM dictionary_blocks ORDER BY kind,first",
    )?;
    let mut rows = select.query([])?;
    let mut previous: Option<(String, String)> = None;
    let mut insert = c.prepare("INSERT INTO dictionary_expected VALUES(?1,?2)")?;
    while let Some(row) = rows.next()? {
        let block: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        let first: Option<String> = row.get(2)?;
        let last: Option<String> = row.get(3)?;
        let bytes = row
            .get_ref(5)?
            .as_blob()
            .map_err(|_| Error::operation("invalid lookup dictionary payload"))?;
        let values = super::codec::decode(bytes, row.get(4)?)?;
        let digest: Option<Vec<u8>> = row.get(6)?;
        if let Some(digest) = digest {
            if first.is_some()
                || last.is_some()
                || values.len() != 1
                || values[0].1.len() <= 65536
                || digest.as_slice() != blake3::hash(values[0].1.as_bytes()).as_bytes()
            {
                return Err(Error::operation(
                    "lookup dictionary overflow address mismatch",
                ));
            }
            c.execute(
                "INSERT INTO dictionary_expected_large VALUES(?1,?2)",
                params![kind, values[0].1],
            )?;
        } else {
            if values.first().map(|r| &r.1) != first.as_ref()
                || values.last().map(|r| &r.1) != last.as_ref()
                || previous.as_ref().is_some_and(|(k, last)| {
                    k == &kind && first.as_ref().is_some_and(|first| last >= first)
                })
            {
                return Err(Error::operation(
                    "lookup dictionary bounds or ordering mismatch",
                ));
            }
            previous = Some((
                kind,
                last.ok_or_else(|| Error::operation("missing dictionary bound"))?,
            ));
        }
        for (id, _) in values {
            insert.execute(params![id, block])?;
        }
    }
    drop(rows);
    drop(select);
    drop(insert);
    let invalid:bool=c.query_row("SELECT EXISTS(SELECT * FROM dictionary_expected EXCEPT SELECT * FROM dictionary_ids) OR EXISTS(SELECT * FROM dictionary_ids EXCEPT SELECT * FROM dictionary_expected)",[],|r|r.get(0))?;
    c.execute_batch("DROP TABLE dictionary_expected; DROP TABLE dictionary_expected_large;")?;
    if invalid {
        return Err(Error::operation("lookup dictionary ID routing mismatch"));
    }
    Ok(())
}
