// Purpose: 복수 선언 위치를 읽기 바이트 예산 안에서 객체에 부착.
// Pattern: Storage Adapter.
// Usage: 공개 symbol 행을 만드는 index와 query가 호출.
// Related: crates/kodstar-core/src/infrastructure/index/mod.rs, crates/kodstar-core/src/domain/symbols.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::source::SourceSpan;
use crate::domain::symbols::{DefinitionLocation, Symbol};
use crate::infrastructure::cache::paths::{encoding, path_from_bytes};
use rusqlite::Connection;

pub(crate) fn attach(c: &Connection, symbol: &mut Symbol, count: u64) -> Result<()> {
    let id = symbol.id;
    if count < 2 {
        return Ok(());
    }
    let mut statement=c.prepare("SELECT f.display_path,f.path,s.start_byte,s.end_byte,s.line,s.column_byte,s.end_line,s.end_column_byte FROM symbol_identities g JOIN symbols s ON s.id=g.symbol_id JOIN files f ON f.id=s.file_id WHERE g.canonical_id=?1 ORDER BY f.path,s.start_byte")?;
    let mut rows = statement.query([id])?;
    let limit = c.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize;
    let mut bytes = usize::try_from(count)
        .ok()
        .and_then(|n| n.checked_mul(std::mem::size_of::<DefinitionLocation>()))
        .filter(|n| *n <= limit)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Limit,
                "symbol definitions exceed the working-memory read limit",
            )
        })?;
    symbol
        .definitions
        .try_reserve_exact(count as usize)
        .map_err(|e| Error::caused_by("allocate symbol definition locations", e))?;
    while let Some(row) = rows.next()? {
        let file: String = row.get(0)?;
        let raw: Vec<u8> = row.get(1)?;
        bytes = bytes
            .saturating_add(file.len())
            .saturating_add(raw.len().saturating_mul(2));
        if bytes > limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "symbol definitions exceed the working-memory read limit",
            ));
        }
        symbol.definitions.push(DefinitionLocation {
            file,
            file_encoding: encoding(&path_from_bytes(&raw)?),
            span: SourceSpan {
                start_byte: row.get(2)?,
                end_byte: row.get(3)?,
                line: row.get(4)?,
                column: row.get(5)?,
                end_line: row.get(6)?,
                end_column: row.get(7)?,
            },
        });
    }
    Ok(())
}
