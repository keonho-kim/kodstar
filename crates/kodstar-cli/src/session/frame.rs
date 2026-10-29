// Purpose: JSON 프레임을 제한된 메모리로 직렬화하고 큰 프레임의 임시 파일을 회수.
// Pattern: Bounded Buffer and RAII.
// Usage: 세션 출력 queue가 프레임을 소유하고 transport가 순차 읽음.
// Related: crates/kodstar-cli/src/session/output.rs, crates/kodstar-cli/src/session/transport.rs.

use serde::Serialize;
use std::fs::File;
use std::io::{self, Read, Seek, Write};

const MEMORY: usize = 64 * 1024;

pub(super) struct Frame {
    memory: Vec<u8>,
    spool: Option<File>,
    offset: usize,
}

impl Frame {
    pub fn encode(value: &impl Serialize) -> io::Result<Self> {
        let mut frame = Self {
            memory: Vec::new(),
            spool: None,
            offset: 0,
        };
        serde_json::to_writer(&mut frame, value).map_err(io::Error::other)?;
        frame.write_all(b"\n")?;
        frame.flush()?;
        if let Some(spool) = &mut frame.spool {
            spool.rewind()?;
            frame.memory = Vec::new();
        }
        Ok(frame)
    }

    fn spill(&mut self) -> io::Result<()> {
        // The unnamed file has no directory entry, including after process termination.
        let mut spool = tempfile::tempfile()?;
        spool.write_all(&self.memory)?;
        self.memory.clear();
        self.spool = Some(spool);
        Ok(())
    }
}

impl Write for Frame {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.spool.is_none() && self.memory.len().saturating_add(bytes.len()) > MEMORY {
            self.spill()?;
        }
        if self.spool.is_some() {
            if self.memory.len().saturating_add(bytes.len()) > MEMORY {
                self.flush()?;
            }
            if bytes.len() >= MEMORY {
                self.spool
                    .as_mut()
                    .ok_or_else(|| io::Error::other("missing frame spool"))?
                    .write_all(bytes)?;
            } else {
                self.memory.extend_from_slice(bytes);
            }
        } else {
            let needed = self.memory.len() + bytes.len();
            if needed > self.memory.capacity() {
                let capacity = needed.next_power_of_two().min(MEMORY);
                self.memory.reserve_exact(capacity - self.memory.len());
            }
            self.memory.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if let Some(spool) = &mut self.spool {
            spool.write_all(&self.memory)?;
            self.memory.clear();
        }
        Ok(())
    }
}

impl Read for Frame {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if let Some(spool) = &mut self.spool {
            spool.read(bytes)
        } else {
            let count = bytes.len().min(self.memory.len() - self.offset);
            bytes[..count].copy_from_slice(&self.memory[self.offset..self.offset + count]);
            self.offset += count;
            Ok(count)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spilled_frames_are_lossless_and_removed_on_drop() -> io::Result<()> {
        let value = "☆".repeat(MEMORY);
        let mut frame = Frame::encode(&value)?;
        assert!(frame.memory.is_empty());
        use std::os::unix::fs::MetadataExt;
        let spool = frame
            .spool
            .as_ref()
            .ok_or_else(|| io::Error::other("missing spool"))?;
        assert_eq!(spool.metadata()?.nlink(), 0);
        let mut encoded = String::new();
        frame.read_to_string(&mut encoded)?;
        assert_eq!(serde_json::from_str::<String>(&encoded)?, value);
        drop(frame);
        Ok(())
    }

    #[test]
    fn escaped_fragments_cross_buffer_boundaries_without_retained_memory() -> io::Result<()> {
        let value = vec!["\"한글\n\\"; 20_000];
        let mut frame = Frame::encode(&value)?;
        assert_eq!(frame.memory.capacity(), 0);
        let mut output = Vec::new();
        frame.read_to_end(&mut output)?;
        assert_eq!(output.last(), Some(&b'\n'));
        assert_eq!(serde_json::from_slice::<Vec<String>>(&output)?, value);
        Ok(())
    }
}
