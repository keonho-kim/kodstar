// Purpose: 스캔 중 이전 파일 상태와 추출 fingerprint의 고정 SQL 조회.
// Pattern: Storage Reader.
// Usage: discovery가 scan reader 수명 동안 하나를 소유.
// Related: crates/kodstar-core/src/application/analysis/discovery.rs, crates/kodstar-core/src/infrastructure/index/write.rs.

use super::FileRecord;
use crate::domain::diagnostics::Result;
use crate::domain::facts::Fingerprints;
use crate::infrastructure::cache::paths::path_bytes;
use rusqlite::{Connection, OptionalExtension, Statement};
use std::path::Path;

pub(crate) struct PreviousFile {
    pub(crate) size: u64,
    pub(crate) hash: String,
    pub(crate) language: String,
    pub(crate) status: String,
    pub(crate) base_module: String,
    pub(crate) package: String,
    pub(crate) configuration: bool,
    pub(crate) fingerprints: Fingerprints,
}

impl PreviousFile {
    pub(crate) fn same_metadata(&self, record: &FileRecord) -> bool {
        self.hash == record.content_hash && self.size == record.size && self.same_context(record)
    }
    pub(crate) fn same_context(&self, record: &FileRecord) -> bool {
        self.language == record.language
            && self.status == record.status
            && self.base_module == record.module
            && self.package == record.package
    }
}

pub(crate) struct PreviousFiles<'conn> {
    statement: Statement<'conn>,
}
impl<'conn> PreviousFiles<'conn> {
    pub(crate) fn new(connection: &'conn Connection) -> Result<Self> {
        Ok(Self { statement: connection.prepare("SELECT lower(hex(facts_hash)),lower(hex(environment_hash)),size,lower(hex(content_hash)),
            language,status,path_module,path_package,EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=files.id AND b.kind='configuration') FROM files WHERE path=?1")? })
    }
    pub(crate) fn get(&mut self, path: &Path) -> Result<Option<PreviousFile>> {
        Ok(self
            .statement
            .query_row([path_bytes(path)], |row| {
                Ok(PreviousFile {
                    fingerprints: Fingerprints {
                        all: row.get(0)?,
                        environment: row.get(1)?,
                    },
                    size: row.get(2)?,
                    hash: row.get(3)?,
                    language: row.get(4)?,
                    status: row.get(5)?,
                    base_module: row.get(6)?,
                    package: row.get(7)?,
                    configuration: row.get(8)?,
                })
            })
            .optional()?)
    }
}
