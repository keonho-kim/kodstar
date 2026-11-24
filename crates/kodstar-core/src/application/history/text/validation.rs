// Purpose: 전체 원문을 보유하지 않는 텍스트 판별과 blob 무결성 확인.
// Pattern: Streaming Validation.
// Usage: diff가 표시 범위를 계산하기 전에 양쪽 내용을 확인.
// Related: crates/kodstar-core/src/application/history/text.rs, crates/kodstar-core/src/infrastructure/history/content/blobs.rs.

use crate::domain::{cancellation::Cancellation, diagnostics::Result};
use crate::infrastructure::history::content::blobs;
use rusqlite::Connection;
use std::io::{self, Write};

#[derive(Clone, Copy)]
pub(super) enum Content {
    Absent,
    Text { id: i64, size: u64 },
    Omitted(&'static str),
}

#[derive(Default)]
struct TextCheck {
    tail: [u8; 4],
    pending: usize,
    binary: bool,
}

impl Write for TextCheck {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if input.iter().any(|b| *b < 0x20 && !b"\n\r\t".contains(b)) {
            self.binary = true;
        }
        let mut bytes = input;
        if !self.binary && self.pending > 0 {
            // An incomplete suffix accepted by from_utf8 starts with a valid 2–4 byte lead.
            let required = if self.tail[0] < 0xe0 {
                2
            } else if self.tail[0] < 0xf0 {
                3
            } else {
                4
            };
            let take = bytes.len().min(required - self.pending);
            self.tail[self.pending..self.pending + take].copy_from_slice(&bytes[..take]);
            self.pending += take;
            bytes = &bytes[take..];
            if self.pending < required {
                return Ok(input.len());
            }
            self.binary |= std::str::from_utf8(&self.tail[..required]).is_err();
            self.pending = 0;
        }
        if !self.binary
            && let Err(error) = simdutf8::compat::from_utf8(bytes)
        {
            if error.error_len().is_some() {
                self.binary = true;
            } else {
                let suffix = &bytes[error.valid_up_to()..];
                self.pending = suffix.len();
                self.tail[..suffix.len()].copy_from_slice(suffix);
            }
        }
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn read(
    connection: &Connection,
    id: Option<i64>,
    max: usize,
    cancel: &Cancellation,
) -> Result<Content> {
    let Some(id) = id else {
        return Ok(Content::Absent);
    };
    let size = connection.query_row("SELECT size FROM blobs WHERE id=?1", [id], |r| {
        r.get::<_, u64>(0)
    })?;
    if size > max as u64 {
        return Ok(Content::Omitted("display_size_limit"));
    }
    let mut check = TextCheck::default();
    blobs::write(connection, id, &mut check, cancel)?;
    if check.binary || check.pending > 0 {
        return Ok(Content::Omitted("binary_content"));
    }
    Ok(Content::Text { id, size })
}
