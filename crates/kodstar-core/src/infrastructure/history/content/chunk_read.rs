// Purpose: 물리 청크와 독립 기준의 크기·주소·개행을 확인한 제한 복원.
// Pattern: Storage Reader.
// Usage: blob·구간 reader·공유 쓰기·doctor가 내용 주소를 검증.
// Related: crates/kodstar-core/src/infrastructure/history/content/encoding.rs, crates/kodstar-core/src/infrastructure/history/content/chunks.rs, crates/kodstar-core/src/infrastructure/history/content/chunk_read/cache.rs.

use super::{chunks::line_info, encoding};
use crate::domain::diagnostics::{Error, Result};
use rusqlite::{Connection, Row};

mod cache;

const COLUMNS: &str =
    "d.hash,d.size,d.codec,d.base,d.lines,d.first_newline,d.last_newline,d.content";

pub(in crate::infrastructure::history) struct Reader<'conn> {
    connection: &'conn Connection,
    workspace: ReadWorkspace,
}

#[derive(Default)]
/// Operation-owned codec state and at most one bounded, verified base chunk.
pub(crate) struct ReadWorkspace {
    decoder: encoding::Decoder,
    base: cache::Base,
}

impl<'conn> Reader<'conn> {
    pub fn new(connection: &'conn Connection) -> Self {
        Self {
            connection,
            workspace: ReadWorkspace::default(),
        }
    }

    pub fn load(&mut self, id: i64) -> Result<Vec<u8>> {
        self.workspace.load(self.connection, id)
    }
}

impl ReadWorkspace {
    pub(in crate::infrastructure::history) fn load(
        &mut self,
        connection: &Connection,
        id: i64,
    ) -> Result<Vec<u8>> {
        let mut statement = connection
            .prepare_cached(&format!("SELECT {COLUMNS} FROM chunk_data d WHERE d.id=?1"))?;
        let mut rows = statement.query([id])?;
        let row = rows.next()?.ok_or_else(invalid)?;
        let base: Option<i64> = row.get(3)?;
        if let Some(base) = base {
            if base >= id {
                return Err(invalid());
            }
            let mut select = connection
                .prepare_cached(&format!("SELECT {COLUMNS} FROM chunk_data d WHERE d.id=?1"))?;
            let mut bases = select.query([base])?;
            let base_row = bases.next()?.ok_or_else(invalid)?;
            if base_row.get::<_, Option<i64>>(3)?.is_some() {
                return Err(invalid());
            }
            let limit = (connection
                .limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)?
                .max(0) as usize
                / 64)
                .min(encoding::MAX);
            let dictionary = self.base.read(base_row, &mut self.decoder, limit)?;
            return decode(row, &dictionary, &mut self.decoder);
        }
        decode(row, &[], &mut self.decoder)
    }
}

fn decode(row: &Row<'_>, dictionary: &[u8], decoder: &mut encoding::Decoder) -> Result<Vec<u8>> {
    let bytes = decoder.decode(
        row.get_ref(7)?.as_blob().map_err(|_| invalid())?,
        row.get(2)?,
        row.get(1)?,
        dictionary,
    )?;
    let hash = row.get_ref(0)?.as_blob().map_err(|_| invalid())?;
    if blake3::hash(&bytes).as_bytes().as_slice() != hash
        || line_info(&bytes) != (row.get(4)?, row.get(5)?, row.get(6)?)
    {
        return Err(invalid());
    }
    Ok(bytes)
}

fn invalid() -> Error {
    Error::operation("history chunk checksum mismatch: size, base, address or line metadata")
}
