// Purpose: 조회 의존성을 파일·key별 제한된 차이 정수 블록으로 저장.
// Pattern: Storage Adapter.
// Usage: resolver publication과 index 연결 초기화가 호출.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs, crates/kodstar-core/src/infrastructure/index/dependencies/schema.sql.

mod codec;
mod selection;
pub(crate) use selection::remove_selected;
#[cfg(test)]
mod tests;

use crate::domain::diagnostics::Result;
use rusqlite::{Connection, functions::FunctionFlags, params};

pub(crate) fn register(connection: &Connection) -> rusqlite::Result<()> {
    connection.create_scalar_function(
        "kodstar_postings",
        2,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |context| {
            let bytes = context.get_raw(0).as_blob()?;
            let count = context.get::<usize>(1)?;
            codec::json(bytes, count)
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
        },
    )
}

pub(crate) fn verify(connection: &Connection) -> Result<()> {
    let invalid: bool = connection.query_row(
        "SELECT EXISTS(
        SELECT 1 FROM lookup_blocks b,json_each(kodstar_postings(b.payload,b.count)) j
        LEFT JOIN facts f ON f.id=json_extract(j.value,'$[0]')
        WHERE f.id IS NULL OR f.file_id<>b.file_id)",
        [],
        |r| r.get(0),
    )?;
    if invalid {
        return Err(crate::domain::diagnostics::Error::operation(
            "lookup dependency references a missing or different source file",
        ));
    }
    let duplicate: bool = connection.query_row(
        "SELECT EXISTS(
        SELECT 1 FROM fact_lookups GROUP BY key_id,fact_id HAVING count(*)<>1)",
        [],
        |r| r.get(0),
    )?;
    if duplicate {
        return Err(crate::domain::diagnostics::Error::operation(
            "duplicate lookup dependency across blocks",
        ));
    }
    Ok(())
}

pub(crate) fn publish(connection: &Connection) -> Result<()> {
    super::dictionary::publish(connection)?;

    connection.execute_batch(
        "CREATE TEMP TABLE dependency_rows(key_id INTEGER,file_id INTEGER,fact_id INTEGER,found INTEGER,
            PRIMARY KEY(key_id,file_id,fact_id)) WITHOUT ROWID;
        INSERT INTO dependency_rows SELECT k.key_id,f.file_id,d.fact_id,d.found
        FROM resolution_dependencies d JOIN dictionary_pending k ON k.kind=d.kind AND k.lookup_key=d.lookup_key
        JOIN facts f ON f.id=d.fact_id;",
    )?;
    // A fact selection may leave other consumers of the same key/file block alive.
    // Merge those postings on disk before replacing the touched key/file groups.
    connection.execute_batch("INSERT OR IGNORE INTO dependency_rows
        SELECT b.key_id,b.file_id,json_extract(j.value,'$[0]'),json_extract(j.value,'$[1]')
        FROM (SELECT DISTINCT key_id,file_id FROM dependency_rows) selected
        CROSS JOIN lookup_rows b ON b.key_id=selected.key_id AND b.file_id=selected.file_id
        CROSS JOIN json_each(kodstar_postings(b.payload,b.count)) j;
        DELETE FROM lookup_rows WHERE (key_id,file_id) IN (SELECT key_id,file_id FROM dependency_rows);")?;

    let mut select = connection.prepare(
        "SELECT key_id,file_id,fact_id,found FROM dependency_rows ORDER BY key_id,file_id,fact_id",
    )?;
    let mut rows = select.query([])?;
    let mut insert = connection.prepare(
        "INSERT INTO lookup_rows(key_id,file_id,part,count,payload) VALUES(?1,?2,?3,?4,?5)",
    )?;
    let mut current = None;
    let mut part = 0;
    let mut previous = 0;
    let mut count = 0;
    let mut bytes = Vec::with_capacity(codec::MAX_COUNT * 10);
    while let Some(row) = rows.next()? {
        let key = (row.get::<_, i64>(0)?, row.get::<_, i64>(1)?);
        if current != Some(key) || count == codec::MAX_COUNT {
            if let Some((key_id, file)) = current {
                insert.execute(params![key_id, file, part, count, bytes])?;
            }
            part = if current == Some(key) { part + 1 } else { 0 };
            current = Some(key);
            previous = 0;
            count = 0;
            bytes.clear();
        }
        let fact = row.get(2)?;
        codec::push(&mut bytes, fact, previous, row.get(3)?)?;
        previous = fact;
        count += 1;
    }
    if let Some((key, file)) = current {
        insert.execute(params![key, file, part, count, bytes])?;
    }

    drop(rows);
    drop(select);
    drop(insert);
    connection.execute_batch("DROP TABLE dependency_rows; DROP TABLE dictionary_pending;")?;
    Ok(())
}
