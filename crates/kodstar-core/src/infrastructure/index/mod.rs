// Purpose: SQLite connection·저장 버전과 공개 결과 행 읽기.
// Pattern: Storage Adapter.
// Usage: 분석·조회 실행이 connection을 열고 빌림.
// Related: crates/kodstar-core/src/infrastructure/index/schema.sql, crates/kodstar-core/src/infrastructure/index/write.rs.

//! SQLite resource ownership. Writers publish facts and relationships in one transaction.

mod definitions;
mod fact_statements;
mod facts;
mod lisp_metadata;
mod previous;
mod reader;
mod runtime;
mod statistics;
mod write;

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, Row};

use crate::domain::diagnostics::{Coverage, Error, Result};
use crate::domain::relations::RELATION_REGISTRY_VERSION;
use crate::domain::source::SourceSpan;
use crate::domain::symbols::Symbol;
use crate::infrastructure::cache::paths::{encoding, path_bytes, path_from_bytes};

pub(crate) use facts::{FactWriter, FileFacts};
pub(crate) use previous::PreviousFiles;
pub(crate) use reader::{SymbolReader, symbol};
pub(crate) use runtime::{limit_reads, observe_cancellation, stop_observing};
pub(crate) use write::{FileRecord, clear_file};

pub(crate) const STORAGE_VERSION: u32 = 7;
pub(crate) const EXTRACTION_VERSION: u32 = 42;
const APPLICATION_ID: u32 = 0x4b535441;

pub(crate) const SYMBOL_COLUMNS: &str = "
    COALESCE((SELECT canonical_id FROM symbol_identities WHERE symbol_id=s.id),s.id),
    s.logical_key,s.name,s.qualified_name,s.kind,s.language,f.display_path,
    s.start_byte,s.end_byte,s.line,s.column_byte,s.end_line,s.end_column_byte,
    s.signature,s.condition,
    (SELECT canonical_id FROM symbol_identities WHERE symbol_id=s.owner_id),
    f.path,f.module,f.package,s.scope_id,
    (SELECT COUNT(*) FROM symbol_identities WHERE canonical_id=
        COALESCE((SELECT canonical_id FROM symbol_identities WHERE symbol_id=s.id),s.id))";

pub(crate) fn open(
    path: &Path,
    root: &Path,
    writable: bool,
    cache_bytes: usize,
) -> Result<Connection> {
    open_mode(path, root, writable, writable, cache_bytes)
}

pub(crate) fn open_existing_writer(
    path: &Path,
    root: &Path,
    cache_bytes: usize,
) -> Result<Connection> {
    open_mode(path, root, true, false, cache_bytes)
}

fn open_mode(
    path: &Path,
    root: &Path,
    writable: bool,
    create: bool,
    cache_bytes: usize,
) -> Result<Connection> {
    if !create && !path.is_file() {
        return Err(Error::not_found("no index found; run kodstar build first"));
    }
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(Error::operation("refusing symlink index database"));
    }
    let flags = if create {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } else if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let mut connection = Connection::open_with_flags(path, flags)
        .map_err(|error| Error::caused_by(format!("open index {}", path.display()), error))?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "trusted_schema", false)?;
    connection.pragma_update(None, "temp_store", "FILE")?;
    let cache_kib = i64::try_from(cache_bytes.div_ceil(1024))
        .map_err(|_| Error::argument("DB cache budget is too large"))?;
    let temporary_kib = cache_kib / 4;
    connection.pragma_update(Some("main"), "cache_size", -(cache_kib - temporary_kib))?;
    connection.pragma_update(Some("temp"), "cache_size", -temporary_kib)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let application: u32 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if create && version == 0 && application == 0 {
        let existing: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        if existing != 0 {
            return Err(Error::operation(
                "unrecognized database in cache; refusing to modify it",
            ));
        }
        let transaction = connection.transaction()?;
        transaction.execute_batch(include_str!("schema.sql"))?;
        transaction.execute("INSERT INTO metadata(singleton,root,engine_version,extraction_version,relation_registry_version) VALUES(1,?1,?2,?3,?4)",
            rusqlite::params![path_bytes(root), crate::VERSION, EXTRACTION_VERSION, RELATION_REGISTRY_VERSION])?;
        transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
        transaction.pragma_update(None, "user_version", STORAGE_VERSION)?;
        transaction.commit()?;
    } else if version != STORAGE_VERSION || application != APPLICATION_ID {
        return Err(Error::operation(format!(
            "incompatible index storage version {version}; explicitly clean this cache and run kodstar build"
        )));
    }
    let stored: Vec<u8> =
        connection.query_row("SELECT root FROM metadata WHERE singleton=1", [], |row| {
            row.get(0)
        })?;
    if stored != path_bytes(root) {
        return Err(Error::operation(
            "index root does not match the selected project",
        ));
    }
    if writable {
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
    }
    Ok(connection)
}

pub(crate) fn require_committed(connection: &Connection) -> Result<(i64, u64)> {
    let metadata: (i64, u64) = connection.query_row(
        "SELECT generation,indexed_at FROM metadata WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if metadata.0 == 0 {
        return Err(Error::not_found(
            "no completed index found; run kodstar build first",
        ));
    }
    Ok(metadata)
}

pub(crate) fn coverage(connection: &Connection) -> Result<Coverage> {
    statistics::read(connection)
}

pub(crate) fn refresh_coverage(connection: &Connection, generation: i64) -> Result<()> {
    statistics::refresh(connection, generation)
}

pub(crate) fn symbol_from_row(row: &Row<'_>) -> rusqlite::Result<Symbol> {
    let raw: Vec<u8> = row.get(16)?;
    let path = path_from_bytes(&raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(16, rusqlite::types::Type::Blob, Box::new(error))
    })?;
    Ok(Symbol {
        id: row.get(0)?,
        logical_key: row.get(1)?,
        name: row.get(2)?,
        qualified_name: row.get(3)?,
        kind: row.get(4)?,
        language: row.get(5)?,
        file: row.get(6)?,
        file_encoding: encoding(&path),
        definition: SourceSpan {
            start_byte: row.get(7)?,
            end_byte: row.get(8)?,
            line: row.get(9)?,
            column: row.get(10)?,
            end_line: row.get(11)?,
            end_column: row.get(12)?,
        },
        definitions: Vec::new(),
        signature: row.get(13)?,
        condition: row.get(14)?,
        owner: row.get(15)?,
        module: row.get(17)?,
        package: row.get(18)?,
        scope: row.get(19)?,
    })
}

pub(crate) fn complete_symbol(
    connection: &Connection,
    symbol: &mut Symbol,
    definitions: u64,
) -> Result<()> {
    definitions::attach(connection, symbol, definitions)?;
    lisp_metadata::attach(connection, symbol)
}
