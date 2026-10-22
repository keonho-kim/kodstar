// Purpose: 영구 이력의 명시적 전환·재압축·페이지 정리 조정.
// Pattern: Explicit Migration.
// Usage: history upgrade만 저장소 잠금 아래 호출.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/legacy.rs, crates/kodstar-core/src/infrastructure/history/content/chunks.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::Connection;

mod legacy;
mod repack;
#[cfg(test)]
mod tests;
pub(super) mod verification;

pub(super) fn run(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    super::content::chunks::register(connection)?;
    crate::infrastructure::index::observe_cancellation(connection, cancel)?;
    let transaction = connection.unchecked_transaction()?;
    let outcome = legacy::convert(&transaction, cancel);
    crate::infrastructure::index::stop_observing(&transaction)?;
    if let Err(error) = outcome {
        transaction.finish()?;
        return Err(error);
    }
    cancel.check()?;
    transaction.pragma_update(None, "user_version", super::VERSION)?;
    transaction.commit()?;
    Ok(())
}

pub(super) fn repack(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    known_schema(connection)?;
    repack::run(connection, cancel).map_err(|cause| {
        let kind = cause.kind;
        let mut error = Error::caused_by(
            "history format is 4; repacking did not finish; retry kodstar history upgrade",
            cause,
        );
        error.kind = kind;
        error
    })
}

pub(super) fn compact(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    cancel.check()?;
    crate::infrastructure::index::observe_cancellation(connection, cancel)?;
    let outcome = connection.execute_batch("VACUUM;");
    crate::infrastructure::index::stop_observing(connection)?;
    outcome.map_err(|e| {
        let cause = Error::from(e);
        let kind = cause.kind;
        let mut error = Error::caused_by(
            "history format is 4; compaction did not finish; retry kodstar history upgrade",
            cause,
        );
        error.kind = kind;
        error
    })?;
    cancel.check()
}

fn known_schema(connection: &Connection) -> Result<u32> {
    let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if !(1..=4).contains(&version) {
        return Err(Error::operation("unsupported legacy history format"));
    }
    let mut statement =
        connection.prepare("SELECT name,type FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let known = kind == "table"
            && matches!(
                name.as_str(),
                "repository"
                    | "checkpoints"
                    | "branches"
                    | "names"
                    | "blobs"
                    | "entries"
                    | "working"
                    | "checkout"
                    | "checkout_files"
            )
            || kind == "table" && version == 1 && name == "chunks"
            || kind == "table"
                && version >= 2
                && matches!(name.as_str(), "chunk_data" | "chunk_refs")
            || kind == "view" && version >= 2 && name == "chunks"
            || kind == "index" && name == "entries_path";
        if !known {
            return Err(Error::operation(
                "unknown history schema was preserved; upgrade refused",
            ));
        }
    }
    if version == 4 {
        columns(
            connection,
            "chunk_data",
            &[
                ("id", "INTEGER"),
                ("hash", "BLOB"),
                ("size", "INTEGER"),
                ("codec", "INTEGER"),
                ("base", "INTEGER"),
                ("lines", "INTEGER"),
                ("first_newline", "INTEGER"),
                ("last_newline", "INTEGER"),
                ("content", "BLOB"),
            ],
        )?;
        columns(
            connection,
            "chunk_refs",
            &[
                ("blob", "INTEGER"),
                ("part", "INTEGER"),
                ("start", "INTEGER"),
                ("chunk", "INTEGER"),
            ],
        )?;
    } else if version >= 2 {
        let mut expected = vec![
            ("hash", "BLOB"),
            ("lines", "INTEGER"),
            ("first_newline", "INTEGER"),
            ("last_newline", "INTEGER"),
            ("content", "BLOB"),
        ];
        if version == 3 {
            expected.insert(0, ("id", "INTEGER"));
        }
        columns(connection, "chunk_data", &expected)?;
        columns(
            connection,
            "chunk_refs",
            &[("blob", "INTEGER"), ("part", "INTEGER"), ("hash", "BLOB")],
        )?;
    } else {
        columns(
            connection,
            "chunks",
            &[
                ("blob", "INTEGER"),
                ("part", "INTEGER"),
                ("content", "BLOB"),
            ],
        )?;
    }
    Ok(version)
}

fn columns(connection: &Connection, table: &str, expected: &[(&str, &str)]) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_xinfo({table})"))?;
    let mut rows = statement.query([])?;
    for &(name, kind) in expected {
        let row = rows
            .next()?
            .ok_or_else(|| Error::operation("unsupported legacy chunk schema"))?;
        if row.get::<_, String>(1)? != name
            || row.get::<_, String>(2)? != kind
            || row.get::<_, u32>(6)? != 0
        {
            return Err(Error::operation(
                "unknown history chunk columns were preserved; upgrade refused",
            ));
        }
    }
    if rows.next()?.is_some() {
        return Err(Error::operation(
            "unknown history chunk columns were preserved; upgrade refused",
        ));
    }
    Ok(())
}
