// Purpose: 원문 주소 청크의 공유·물리 인코딩·단일 기준 델타와 정규 읽기.
// Pattern: Storage Adapter.
// Usage: blob 작성·복원과 명시적 이력 전환이 제한된 청크를 전달.
// Related: crates/kodstar-core/src/infrastructure/history/content/chunks.sql, crates/kodstar-core/src/infrastructure/history/content/chunk_read.rs.

use super::encoding;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::{Connection, OptionalExtension, Statement, functions::FunctionFlags, params};
use std::io::Read;

pub(in crate::infrastructure::history) const SCHEMA: &str = include_str!("chunks.sql");

pub(in crate::infrastructure::history) struct Writer<'conn> {
    connection: &'conn Connection,
    encoder: encoding::Encoder,
    reader: super::chunk_read::Reader<'conn>,
    content: Statement<'conn>,
    reference: Statement<'conn>,
    matching: Statement<'conn>,
}

impl<'conn> Writer<'conn> {
    pub(in crate::infrastructure::history) fn new(connection: &'conn Connection) -> Result<Self> {
        Ok(Self {
            connection,
            encoder: encoding::Encoder::default(),
            reader: super::chunk_read::Reader::new(connection),
            content: connection.prepare("INSERT INTO chunk_data(hash,size,codec,base,lines,first_newline,last_newline,content) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)")?,
            reference: connection.prepare("INSERT INTO chunk_refs VALUES(?1,?2,?3,?4)")?,
            matching: connection.prepare("SELECT id FROM chunk_data WHERE hash=?1")?,
        })
    }

    pub(in crate::infrastructure::history) fn put(
        &mut self,
        blob: i64,
        part: u64,
        start: u64,
        bytes: &[u8],
        base: Option<i64>,
    ) -> Result<()> {
        let id = self.store(bytes, base)?;
        self.reference(blob, part, start, id)
    }

    pub(in crate::infrastructure::history) fn reference(
        &mut self,
        blob: i64,
        part: u64,
        start: u64,
        id: i64,
    ) -> Result<()> {
        self.reference.execute(params![blob, part, start, id])?;
        Ok(())
    }

    pub(in crate::infrastructure::history) fn store(
        &mut self,
        bytes: &[u8],
        base: Option<i64>,
    ) -> Result<i64> {
        self.store_hashed(bytes, base, &blake3::hash(bytes))
    }

    pub(in crate::infrastructure::history) fn store_hashed(
        &mut self,
        bytes: &[u8],
        base: Option<i64>,
        hash: &blake3::Hash,
    ) -> Result<i64> {
        if bytes.is_empty() || bytes.len() > encoding::MAX {
            return Err(Error::operation("invalid history chunk size"));
        }
        let hash = hash.as_bytes().as_slice();
        let previous: Option<i64> = self.matching.query_row([hash], |r| r.get(0)).optional()?;
        let id = if let Some(id) = previous {
            if self.reader.load(id)? != bytes {
                return Err(Error::operation("stored chunk checksum mismatch"));
            }
            id
        } else {
            let mut chosen = None;
            if bytes.len() >= encoding::MIN_INPUT {
                let packed = self.encoder.encode(bytes, &[])?;
                if packed.len() + encoding::SAVINGS < bytes.len() {
                    chosen = Some(packed);
                }
            }
            let (mut codec, mut selected_base) = (u8::from(chosen.is_some()), None);
            if let Some(base) = base.filter(|_| bytes.len() >= encoding::MIN_INPUT) {
                let independent: bool = self.connection.query_row(
                    "SELECT codec<>2 AND base IS NULL FROM chunk_data WHERE id=?1",
                    [base],
                    |r| r.get(0),
                )?;
                if !independent {
                    return Err(Error::operation(
                        "history delta base is not an independent chunk",
                    ));
                }
                let dictionary = self.reader.load(base)?;
                let delta = self.encoder.encode(bytes, &dictionary)?;
                if delta.len() + encoding::SAVINGS < chosen.as_ref().map_or(bytes.len(), Vec::len) {
                    codec = 2;
                    chosen = Some(delta);
                    selected_base = Some(base);
                }
            }
            let (lines, first, last) = line_info(bytes);
            self.content.execute(params![
                hash,
                bytes.len(),
                codec,
                selected_base,
                lines,
                first,
                last,
                chosen.as_deref().unwrap_or(bytes)
            ])?;
            self.connection.last_insert_rowid()
        };
        Ok(id)
    }
}

pub(in crate::infrastructure::history) fn register(
    connection: &Connection,
) -> rusqlite::Result<()> {
    let decoder = std::cell::RefCell::new(encoding::Decoder::default());
    connection.create_scalar_function(
        "kodstar_content",
        6,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        move |c| {
            let mut decoder = decoder.try_borrow_mut().map_err(|_| {
                rusqlite::Error::UserFunctionError(Box::new(Error::operation(
                    "recursive history decoding",
                )))
            })?;
            let dictionary = if matches!(c.get_raw(3), rusqlite::types::ValueRef::Null) {
                Vec::new()
            } else {
                decoder
                    .decode(c.get_raw(3).as_blob()?, c.get(4)?, c.get(5)?, &[])
                    .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?
            };
            decoder
                .decode(c.get_raw(0).as_blob()?, c.get(1)?, c.get(2)?, &dictionary)
                .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
        },
    )
}

pub(in crate::infrastructure::history) fn line_info(bytes: &[u8]) -> (u64, i64, i64) {
    let lines = memchr::memchr_iter(b'\n', bytes).count() as u64;
    if lines == 0 {
        return (0, -1, -1);
    }
    let first = memchr::memchr(b'\n', bytes).map_or(-1, |i| i as i64);
    let last = memchr::memrchr(b'\n', bytes).map_or(-1, |i| i as i64);
    (lines, first, last)
}

pub(in crate::infrastructure::history) fn read(
    input: &mut impl Read,
    buffer: &mut [u8],
    cancel: &Cancellation,
) -> Result<usize> {
    let mut used = 0;
    while used < buffer.len() {
        cancel.check()?;
        match input.read(&mut buffer[used..]) {
            Ok(0) => break,
            Ok(read) => used += read,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::caused_by("read history input", e)),
        }
    }
    Ok(used)
}
