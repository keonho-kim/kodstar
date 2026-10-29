// Purpose: 형식 4의 모든 청크 주소·참조를 유지하며 Zstd 물리 표현으로 전환.
// Pattern: Transactional Migration.
// Usage: history upgrade transaction이 기존 decoder와 제한된 새 encoder를 연결.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/prior_encoding.rs, crates/kodstar-core/src/infrastructure/history/maintenance/mod.rs.

use super::{prior_encoding, verification};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::history::content::{chunks::line_info, encoding};
use rusqlite::{Connection, Row, params};

#[cfg(test)]
mod tests;

const COLUMNS: &str = "id,hash,size,codec,base,lines,first_newline,last_newline,content";

pub(super) fn convert(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    verification::references(connection)?;
    let mut encoder = encoding::Encoder::default();
    let mut previous: Option<i64> = None;
    loop {
        cancel.check()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM chunk_data WHERE ?1 IS NULL OR id<?1 ORDER BY id DESC LIMIT 1"
        ))?;
        let mut rows = statement.query([previous])?;
        let Some(row) = rows.next()? else {
            break;
        };
        let id: i64 = row.get(0)?;
        let base: Option<i64> = row.get(4)?;
        // Descending IDs keep every base in the old format until all of its users
        // have been converted. The caller's transaction hides mixed representations.
        let dictionary = if let Some(base) = base {
            if base >= id {
                return Err(invalid());
            }
            let mut select =
                connection.prepare(&format!("SELECT {COLUMNS} FROM chunk_data WHERE id=?1"))?;
            let mut bases = select.query([base])?;
            let row = bases.next()?.ok_or_else(invalid)?;
            if row.get::<_, Option<i64>>(4)?.is_some() {
                return Err(invalid());
            }
            decode(row, &[])?
        } else {
            Vec::new()
        };
        let raw = decode(row, &dictionary)?;
        drop(rows);
        drop(statement);
        let (mut codec, mut selected_base, mut chosen) = (0u8, None, None);
        if raw.len() >= encoding::MIN_INPUT {
            let independent = encoder.encode(&raw, &[])?;
            if independent.len() + encoding::SAVINGS < raw.len() {
                codec = 1;
                chosen = Some(independent);
            }
            if let Some(base) = base {
                let delta = encoder.encode(&raw, &dictionary)?;
                if delta.len() + encoding::SAVINGS < chosen.as_ref().map_or(raw.len(), Vec::len) {
                    codec = 2;
                    selected_base = Some(base);
                    chosen = Some(delta);
                }
            }
        }
        connection.execute(
            "UPDATE chunk_data SET codec=?2,base=?3,content=?4 WHERE id=?1",
            params![id, codec, selected_base, chosen.as_deref().unwrap_or(&raw)],
        )?;
        previous = Some(id);
    }
    verification::references(connection)
}

fn decode(row: &Row<'_>, base: &[u8]) -> Result<Vec<u8>> {
    let bytes = prior_encoding::decode(
        row.get_ref(8)?.as_blob().map_err(|_| invalid())?,
        row.get(3)?,
        row.get(2)?,
        base,
    )?;
    if blake3::hash(&bytes).as_bytes().as_slice()
        != row.get_ref(1)?.as_blob().map_err(|_| invalid())?
        || line_info(&bytes) != (row.get(5)?, row.get(6)?, row.get(7)?)
    {
        return Err(invalid());
    }
    Ok(bytes)
}

fn invalid() -> Error {
    Error::operation("history content checksum mismatch; format was preserved")
}
