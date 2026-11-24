// Purpose: 선택한 보존 구간을 원본 행 번호와 한 행 버퍼로 출력.
// Pattern: Streaming Output.
// Usage: 텍스트 diff가 완전한 행 범위 또는 정렬한 원본 행을 전달.
// Related: crates/kodstar-core/src/application/history/text.rs, crates/kodstar-core/src/application/history/text/alignment.rs, crates/kodstar-core/src/infrastructure/history/content/reader.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    history::HistoryRow,
    results::{QueryRow, QuerySink},
};
use crate::infrastructure::{
    cache::paths::encoding,
    history::content::{blobs::CHUNK, reader::Reader},
};
use rusqlite::Connection;
use std::{ops::Range, path::Path};

pub(super) struct Lines<'a> {
    pub path: &'a Path,
    pub side: &'a str,
    pub line: u64,
    pub cancel: &'a Cancellation,
    pub sink: &'a mut dyn QuerySink,
}

impl Lines<'_> {
    pub fn render(&mut self, connection: &Connection, id: i64, range: Range<u64>) -> Result<u64> {
        let mut reader = Reader::verified(connection, id)?;
        let mut buffer = [0; CHUNK];
        let mut text = Vec::new();
        let mut offset = range.start;
        let mut count = 0;
        while offset < range.end {
            let wanted = (range.end - offset).min(CHUNK as u64) as usize;
            let read = reader.read_at(offset, &mut buffer[..wanted], self.cancel)?;
            if read == 0 {
                return Err(Error::operation("text diff range exceeds stored source"));
            }
            let mut bytes = &buffer[..read];
            while !bytes.is_empty() {
                self.cancel.check()?;
                let take = bytes
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(bytes.len(), |i| i + 1);
                text.extend_from_slice(&bytes[..take]);
                bytes = &bytes[take..];
                if text.last() == Some(&b'\n') {
                    self.buffer(&mut text)?;
                    count += 1;
                }
            }
            offset += read as u64;
        }
        if !text.is_empty() {
            self.buffer(&mut text)?;
            count += 1;
        }
        Ok(count)
    }

    fn buffer(&mut self, buffer: &mut Vec<u8>) -> Result<()> {
        let text = String::from_utf8(std::mem::take(buffer))
            .map_err(|e| Error::caused_by("invalid text diff boundary", e))?;
        *buffer = self.emit(text)?.into_bytes();
        buffer.clear();
        Ok(())
    }

    pub(super) fn emit(&mut self, text: String) -> Result<String> {
        self.cancel.check()?;
        let row = QueryRow::History(HistoryRow::DiffLine {
            path: self.path.to_string_lossy().into_owned(),
            path_encoding: encoding(self.path),
            side: self.side.into(),
            line: self.line,
            text,
        });
        self.sink.row(&row)?;
        if let QueryRow::History(HistoryRow::DiffLine { text, .. }) = row {
            self.line += 1;
            return Ok(text);
        }
        unreachable!("the emitted row is a diff line")
    }
}
