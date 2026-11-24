// Purpose: 고정 체크포인트의 native metadata·부재 관측을 바이트 제한 안에서 공유.
// Pattern: Operation Cache.
// Usage: ProjectInputs가 단계 동안 소유하고 파일마다 의존 관측을 다시 기록.
// Related: crates/kodstar-core/src/infrastructure/clang/inputs.rs, crates/kodstar-core/src/infrastructure/history/inputs/mod.rs.

use crate::domain::{
    diagnostics::{ErrorKind, Result},
    native::Observation,
};
use crate::infrastructure::history::inputs::{self, Entry};
use std::{path::Path, rc::Rc};

struct Record {
    path: String,
    entry: Option<Entry>,
    observation: Observation,
}

pub(super) struct Catalog {
    records: Vec<Record>,
    bytes: usize,
    limit: usize,
}

impl Catalog {
    pub fn new(limit: usize) -> Self {
        let overhead = 8 * std::mem::size_of::<Record>();
        let limit = limit.min(16384).saturating_sub(overhead);
        Self {
            records: Vec::with_capacity(if limit > 0 { 8 } else { 0 }),
            bytes: 0,
            limit,
        }
    }

    pub fn get(
        &mut self,
        connection: &Rc<rusqlite::Connection>,
        name: &str,
    ) -> Result<(Option<Entry>, Observation)> {
        if let Some(record) = self.records.iter().rev().find(|r| r.path == name) {
            return Ok((record.entry.clone(), record.observation.clone()));
        }
        let path = Path::new(name);
        let entry = match inputs::Reader::new(connection)?.get(path) {
            Ok(entry) => Some(entry),
            Err(e) if e.kind == ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let observation = if let Some(entry) = &entry {
            Observation::File(entry.content_hash())
        } else {
            super::missing_or_directory(connection, path)?
        };
        // Charge the duplicated path, digest, vector storage and Rc metadata.
        let bytes = name.len().saturating_mul(3).saturating_add(512);
        if bytes <= self.limit {
            if self.records.len() == 8 || self.bytes + bytes > self.limit {
                self.records.clear();
                self.bytes = 0;
            }
            self.records.push(Record {
                path: name.to_owned(),
                entry: entry.clone(),
                observation: observation.clone(),
            });
            self.bytes += bytes;
        }
        Ok((entry, observation))
    }
}
