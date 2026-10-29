// Purpose: 영구 이력의 명시적 전환·재압축·페이지 정리 조정.
// Pattern: Explicit Migration.
// Usage: history upgrade만 저장소 잠금 아래 호출.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/legacy.rs, crates/kodstar-core/src/infrastructure/history/content/chunks.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::Connection;

mod compressed;
mod legacy;
mod metadata;
mod prior_encoding;
mod repack;
#[cfg(test)]
mod tests;
pub(super) mod verification;

pub(super) fn run(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    super::content::chunks::register(connection)?;
    crate::infrastructure::index::observe_cancellation(connection, cancel)?;
    let version = known_schema(connection)?;
    if version == super::VERSION {
        return Err(Error::operation("history is already in the current format"));
    }
    // The owning transaction rebuilds referenced tables without firing delete cascades.
    // Foreign keys are checked before commit and restored on both success and failure.
    connection.pragma_update(None, "foreign_keys", false)?;
    let outcome = (|| -> Result<()> {
        let transaction = connection.unchecked_transaction()?;
        let conversion = (|| -> Result<()> {
            match version {
                4 => compressed::convert(&transaction, cancel)?,
                1..=3 => legacy::convert(&transaction, cancel)?,
                5 => {}
                _ => return Err(Error::operation("unsupported legacy history format")),
            }
            metadata::convert(&transaction, cancel)?;
            verification::verify(&transaction, cancel)?;
            cancel.check()?;
            Ok(())
        })();
        crate::infrastructure::index::stop_observing(&transaction)?;
        if let Err(error) = conversion {
            transaction.finish().map_err(|rollback| {
                Error::caused_by(
                    format!("history conversion failed ({error}); rollback failed"),
                    rollback,
                )
            })?;
            return Err(error);
        }
        transaction.pragma_update(None, "user_version", super::VERSION)?;
        transaction.commit()?;
        Ok(())
    })();
    let stop = crate::infrastructure::index::stop_observing(connection);
    let restore = connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(Error::from);
    if let Err(error) = outcome {
        stop.and(restore).map_err(|cleanup| {
            Error::caused_by(
                format!("history conversion failed ({error}); cleanup failed"),
                cleanup,
            )
        })?;
        return Err(error);
    }
    stop?;
    restore
}

pub(super) fn repack(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    known_schema(connection)?;
    repack::run(connection, cancel).map_err(|cause| {
        let kind = cause.kind;
        let mut error = Error::caused_by(
            format!(
                "history format is {}; repacking did not finish; retry kodstar history upgrade",
                super::VERSION
            ),
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
            format!(
                "history format is {}; compaction did not finish; retry kodstar history upgrade",
                super::VERSION
            ),
            cause,
        );
        error.kind = kind;
        error
    })?;
    cancel.check()
}

fn known_schema(connection: &Connection) -> Result<u32> {
    let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if !(1..=super::VERSION).contains(&version) {
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
                    | "paths"
                    | "entry_rows"
                    | "checkout"
                    | "checkout_files"
            )
            || kind == "table" && version == 1 && name == "chunks"
            || kind == "table"
                && version >= 2
                && matches!(name.as_str(), "chunk_data" | "chunk_refs")
            || kind == "view" && version >= 2 && name == "chunks"
            || kind == "index" && matches!(name.as_str(), "entries_path" | "entry_rows_path")
            || version == 6 && kind == "view" && name == "entries"
            || version == 6 && kind == "trigger" && name == "entries_insert";
        if !known {
            return Err(Error::operation(
                "unknown history schema was preserved; upgrade refused",
            ));
        }
    }
    columns(
        connection,
        "blobs",
        &[
            ("id", "INTEGER"),
            ("hash", if version >= 6 { "BLOB" } else { "TEXT" }),
            ("size", "INTEGER"),
        ],
    )?;
    if version < 6 {
        columns(
            connection,
            "entries",
            &[
                ("checkpoint", "INTEGER"),
                ("path", "BLOB"),
                ("blob", "INTEGER"),
                ("mode", "INTEGER"),
                ("kind", "TEXT"),
            ],
        )?;
    }
    if version >= 4 {
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
