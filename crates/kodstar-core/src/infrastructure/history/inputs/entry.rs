// Purpose: 체크포인트 blob의 제한된 읽기와 분석 입력 식별.
// Pattern: Input Adapter.
// Usage: history inputs가 생성하고 analysis discovery·configuration이 소비.
// Related: crates/kodstar-core/src/infrastructure/history/inputs/mod.rs, crates/kodstar-core/src/infrastructure/history/content/blobs.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    languages::Language,
};
use rusqlite::Connection;
use std::{path::PathBuf, rc::Rc};

pub(crate) struct Entry {
    pub relative: PathBuf,
    pub language: Option<Language>,
    pub auxiliary: bool,
    pub detect_prefix: bool,
    pub(super) connection: Rc<Connection>,
    pub(super) blob: i64,
    pub(super) size: u64,
    pub(super) hash: String,
}

impl Entry {
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Reuse only after read has verified the stored bytes against this digest.
    pub fn content_hash(&self) -> &str {
        &self.hash
    }

    pub fn has_file(&self, path: &std::path::Path) -> Result<bool> {
        super::super::paths::relative(path)?;
        let mut statement = self
            .connection
            .prepare("SELECT EXISTS(SELECT 1 FROM snapshot_files WHERE path=?1 AND kind='file')")?;
        Ok(statement.query_row(
            [crate::infrastructure::cache::paths::path_bytes(path)],
            |r| r.get(0),
        )?)
    }

    pub fn detect_from_prefix(&self) -> Result<Option<Language>> {
        let mut prefix = Vec::new();
        let mut statement = self
            .connection
            .prepare("SELECT content FROM chunks WHERE blob=?1 ORDER BY part")?;
        let mut rows = statement.query([self.blob])?;
        while prefix.len() < 4096 {
            let Some(row) = rows.next()? else { break };
            let bytes = row
                .get_ref(0)?
                .as_blob()
                .map_err(|e| Error::caused_by("invalid history chunk", e))?;
            prefix.extend_from_slice(&bytes[..bytes.len().min(4096 - prefix.len())]);
        }
        Ok(crate::infrastructure::languages::detect(
            &self.relative,
            &prefix,
        ))
    }

    pub fn read(&self, max_bytes: usize, cancel: &Cancellation) -> Result<Vec<u8>> {
        super::super::content::blobs::read(&self.connection, self.blob, max_bytes, cancel)
    }
}
