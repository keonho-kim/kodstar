// Purpose: 조건부 fact key를 작은 청크로 해시·저장·대조해 큰 JSON 버퍼를 방지.
// Pattern: Streaming Codec.
// Usage: spill adapter가 동일 프로세스의 정규 key를 정확히 비교.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/spill.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::io::{self, Write};

const CHUNK: usize = 16384;

fn encoded<T: Serialize>(key: &T, output: &mut impl Write, cancel: &Cancellation) -> Result<()> {
    let outcome = serde_json::to_writer(output, key);
    cancel.check()?;
    outcome.map_err(|error| Error::caused_by("encode conditional fact key", error))
}

pub(super) fn digest<T: Serialize>(
    key: &T,
    cancel: &Cancellation,
) -> Result<(blake3::Hash, usize)> {
    struct Digest<'a> {
        hash: blake3::Hasher,
        bytes: usize,
        cancel: &'a Cancellation,
    }
    impl Write for Digest<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.cancel.check().map_err(io::Error::other)?;
            self.hash.update(bytes);
            self.bytes = self
                .bytes
                .checked_add(bytes.len())
                .ok_or_else(|| io::Error::other("key length overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Digest {
        hash: blake3::Hasher::new(),
        bytes: 0,
        cancel,
    };
    encoded(key, &mut output, cancel)?;
    Ok((output.hash.finalize(), output.bytes))
}

pub(super) fn store<T: Serialize>(
    connection: &Connection,
    id: i64,
    key: &T,
    cancel: &Cancellation,
) -> Result<()> {
    struct Chunks<'a> {
        connection: &'a Connection,
        id: i64,
        part: usize,
        bytes: Vec<u8>,
        cancel: &'a Cancellation,
        full: bool,
    }
    impl Write for Chunks<'_> {
        fn write(&mut self, mut bytes: &[u8]) -> io::Result<usize> {
            let length = bytes.len();
            while !bytes.is_empty() {
                self.cancel.check().map_err(io::Error::other)?;
                let count = bytes.len().min(CHUNK - self.bytes.len());
                self.bytes.extend_from_slice(&bytes[..count]);
                bytes = &bytes[count..];
                if self.bytes.len() == CHUNK {
                    self.flush()?;
                }
            }
            Ok(length)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.bytes.is_empty() {
                return Ok(());
            }
            self.connection
                .prepare_cached("INSERT INTO chunks(key,part,content) VALUES(?1,?2,?3)")
                .and_then(|mut statement| {
                    statement.execute(params![self.id, self.part, &self.bytes])
                })
                .map_err(|error| {
                    self.full = matches!(&error, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::DiskFull);
                    io::Error::other(error)
                })?;
            self.part += 1;
            self.bytes.clear();
            Ok(())
        }
    }
    let mut output = Chunks {
        connection,
        id,
        part: 0,
        bytes: Vec::with_capacity(CHUNK),
        cancel,
        full: false,
    };
    let result = encoded(key, &mut output, cancel).and_then(|()| {
        output
            .flush()
            .map_err(|error| Error::caused_by("write conditional key chunk", error))
    });
    cancel.check()?;
    result.map_err(|mut error| {
        if output.full {
            error.kind = ErrorKind::Limit;
        }
        error
    })
}

pub(super) fn matches<T: Serialize>(
    connection: &Connection,
    id: i64,
    key: &T,
    cancel: &Cancellation,
) -> Result<bool> {
    struct Compare<'a> {
        connection: &'a Connection,
        id: i64,
        part: usize,
        bytes: Vec<u8>,
        at: usize,
        different: bool,
        cancel: &'a Cancellation,
    }
    impl Write for Compare<'_> {
        fn write(&mut self, mut bytes: &[u8]) -> io::Result<usize> {
            let length = bytes.len();
            while !bytes.is_empty() {
                self.cancel.check().map_err(io::Error::other)?;
                if self.at == self.bytes.len() {
                    self.bytes = self
                        .connection
                        .prepare_cached("SELECT content FROM chunks WHERE key=?1 AND part=?2")
                        .and_then(|mut statement| {
                            statement.query_row(params![self.id, self.part], |row| row.get(0))
                        })
                        .map_err(io::Error::other)?;
                    if self.bytes.is_empty() || self.bytes.len() > CHUNK {
                        return Err(io::Error::other("invalid conditional key chunk"));
                    }
                    self.at = 0;
                    self.part += 1;
                }
                let count = bytes.len().min(self.bytes.len() - self.at);
                if bytes[..count] != self.bytes[self.at..self.at + count] {
                    self.different = true;
                    return Err(io::Error::other("different conditional key"));
                }
                self.at += count;
                bytes = &bytes[count..];
            }
            Ok(length)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Compare {
        connection,
        id,
        part: 0,
        bytes: Vec::new(),
        at: 0,
        different: false,
        cancel,
    };
    let result = encoded(key, &mut output, cancel);
    if output.different {
        return Ok(false);
    }
    result?;
    Ok(output.at == output.bytes.len())
}
