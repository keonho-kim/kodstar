// Purpose: 취소 가능한 stdio 읽기와 64 KiB JSON 프레임 상한 적용.
// Pattern: Input Adapter.
// Usage: 세션 dispatcher만 stdin을 읽고 완성된 프레임을 소비.
// Related: crates/kodstar-cli/src/session/mod.rs, crates/kodstar-cli/src/signals.rs.

use kodstar_core::{Cancellation, Error, ErrorKind, Result};
const MAX_FRAME: usize = 64 * 1024;
const CHUNK: usize = 8192;
pub(super) enum Frame {
    Data(Vec<u8>),
    TooLong,
}
pub(super) struct Input {
    buffer: Vec<u8>,
    discarding: bool,
}
enum Read {
    Bytes(usize),
    Pending,
    Eof,
}

impl Input {
    pub(super) fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(MAX_FRAME + CHUNK),
            discarding: false,
        }
    }
    pub(super) fn next(&mut self, cancel: &Cancellation) -> Result<Option<Frame>> {
        loop {
            if cancel.is_cancelled() {
                return Err(Error::new(ErrorKind::Cancelled, "session cancelled"));
            }
            if let Some(end) = self.buffer.iter().position(|&byte| byte == b'\n') {
                let mut frame: Vec<_> = self.buffer.drain(..=end).collect();
                if self.discarding {
                    self.discarding = false;
                    continue;
                }
                frame.pop();
                if frame.last() == Some(&b'\r') {
                    frame.pop();
                }
                return Ok(Some(if frame.len() > MAX_FRAME {
                    Frame::TooLong
                } else {
                    Frame::Data(frame)
                }));
            }
            if self.buffer.len() > MAX_FRAME {
                self.buffer.clear();
                if !self.discarding {
                    self.discarding = true;
                    return Ok(Some(Frame::TooLong));
                }
            }
            let mut chunk = [0; CHUNK];
            match read(&mut chunk)? {
                Read::Bytes(count) => self.buffer.extend_from_slice(&chunk[..count]),
                Read::Pending => continue,
                Read::Eof => {
                    return Ok(if self.buffer.is_empty() || self.discarding {
                        None
                    } else {
                        Some(Frame::Data(std::mem::take(&mut self.buffer)))
                    });
                }
            }
        }
    }
}

#[cfg(unix)]
fn read(buffer: &mut [u8]) -> Result<Read> {
    use nix::{
        errno::Errno,
        poll::{PollFd, PollFlags, poll},
        unistd,
    };
    use std::os::fd::AsFd;
    let stdin = std::io::stdin();
    let mut descriptors = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
    match poll(&mut descriptors, 25u16) {
        Ok(0) | Err(Errno::EINTR) => return Ok(Read::Pending),
        Ok(_) => {}
        Err(error) => return Err(Error::caused_by("wait for session input", error)),
    }
    match unistd::read(stdin.as_fd(), buffer) {
        Ok(0) => Ok(Read::Eof),
        Ok(count) => Ok(Read::Bytes(count)),
        Err(Errno::EINTR | Errno::EAGAIN) => Ok(Read::Pending),
        Err(error) => Err(Error::caused_by("read session input", error)),
    }
}
