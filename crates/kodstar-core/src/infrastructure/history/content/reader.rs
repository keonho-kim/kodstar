// Purpose: 검증된 가변 청크의 시작 위치를 조회해 필요한 구간만 제한 복원.
// Pattern: Storage Reader.
// Usage: 텍스트 diff가 각 reader의 마지막 청크만 보유해 구간을 읽음.
// Related: crates/kodstar-core/src/infrastructure/history/content/chunk_read.rs, crates/kodstar-core/src/application/history/text.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::{Connection, Statement, params};

pub(crate) struct Reader<'conn> {
    chunks: super::chunk_read::Reader<'conn>,
    statement: Statement<'conn>,
    blob: i64,
    cached_start: u64,
    cached: Vec<u8>,
    pub size: u64,
}

impl<'conn> Reader<'conn> {
    /// Call after blobs::write/read verifies the whole blob, retaining the repository lock.
    pub fn verified(connection: &'conn Connection, blob: i64) -> Result<Self> {
        let size =
            connection.query_row("SELECT size FROM blobs WHERE id=?1", [blob], |r| r.get(0))?;
        Ok(Self { chunks:super::chunk_read::Reader::new(connection),blob,size,cached_start:0,cached:Vec::new(),
            statement:connection.prepare("SELECT start,chunk FROM chunk_refs WHERE blob=?1 AND start<=?2 ORDER BY start DESC LIMIT 1")? })
    }

    pub fn read_at(
        &mut self,
        mut offset: u64,
        output: &mut [u8],
        cancel: &Cancellation,
    ) -> Result<usize> {
        let wanted = self.size.saturating_sub(offset).min(output.len() as u64) as usize;
        let mut used = 0;
        while used < wanted {
            cancel.check()?;
            if self.cached.is_empty()
                || offset < self.cached_start
                || offset - self.cached_start >= self.cached.len() as u64
            {
                let (start, id): (u64, i64) = self
                    .statement
                    .query_row(params![self.blob, offset], |r| Ok((r.get(0)?, r.get(1)?)))?;
                self.cached.clear();
                self.cached = self.chunks.load(id)?;
                self.cached_start = start;
                if start > self.size || self.cached.len() as u64 > self.size - start {
                    return Err(Error::operation("history chunk exceeds declared blob size"));
                }
            }
            let within = (offset - self.cached_start) as usize;
            if within >= self.cached.len() {
                return Err(Error::operation("history chunk layout has a gap"));
            }
            let take = (wanted - used).min(self.cached.len() - within);
            output[used..used + take].copy_from_slice(&self.cached[within..within + take]);
            used += take;
            offset += take as u64;
        }
        Ok(used)
    }
}
