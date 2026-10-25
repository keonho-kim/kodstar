// Purpose: 단일 stdout writer가 프레임 순서·취소·느린 reader와 fd 복원을 소유.
// Pattern: Single Writer and Backpressure.
// Usage: 세션 scope가 transport thread를 시작하고 종료를 join.
// Related: crates/kodstar-cli/src/session/output.rs, crates/kodstar-cli/src/session/frame.rs.

use super::output::Queued;
use kodstar_core::{Cancellation, Error, ErrorKind, Result};
use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl},
    poll::{PollFd, PollFlags, poll},
    unistd,
};
use std::io::Read;
use std::os::fd::AsFd;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

pub(super) struct Transport {
    receiver: mpsc::Receiver<Queued>,
    cancel: Cancellation,
    closing: Arc<AtomicBool>,
    writer: Writer<std::io::Stdout>,
}

impl Transport {
    pub fn new(
        receiver: mpsc::Receiver<Queued>,
        cancel: Cancellation,
        closing: Arc<AtomicBool>,
    ) -> Result<Self> {
        Ok(Self {
            receiver,
            cancel,
            closing,
            writer: Writer::new(std::io::stdout())?,
        })
    }

    pub fn run(self) -> Result<()> {
        let mut deadline = None;
        loop {
            super::check(&self.cancel)?;
            match self.receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(mut frame) => self.writer.send(
                    &mut frame.frame,
                    &self.cancel,
                    &self.closing,
                    &mut deadline,
                )?,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

struct Writer<F: AsFd> {
    file: F,
    previous: OFlag,
}
impl<F: AsFd> Writer<F> {
    fn new(file: F) -> Result<Self> {
        let previous = OFlag::from_bits_truncate(
            fcntl(file.as_fd(), FcntlArg::F_GETFL)
                .map_err(|e| Error::caused_by("read session output flags", e))?,
        );
        fcntl(
            file.as_fd(),
            FcntlArg::F_SETFL(previous | OFlag::O_NONBLOCK),
        )
        .map_err(|e| Error::caused_by("make session output cancellable", e))?;
        Ok(Self { file, previous })
    }

    fn send(
        &self,
        frame: &mut impl Read,
        cancel: &Cancellation,
        closing: &AtomicBool,
        deadline: &mut Option<Instant>,
    ) -> Result<()> {
        let mut bytes = [0; 8192];
        loop {
            let count = frame
                .read(&mut bytes)
                .map_err(|e| Error::caused_by("read session frame", e))?;
            if count == 0 {
                return Ok(());
            }
            let mut at = 0;
            while at < count {
                super::check(cancel)?;
                if closing.load(Ordering::Acquire) {
                    let end =
                        deadline.get_or_insert_with(|| Instant::now() + Duration::from_millis(500));
                    if Instant::now() >= *end {
                        return Err(Error::new(
                            ErrorKind::Cancelled,
                            "session output did not drain during shutdown",
                        ));
                    }
                }
                match unistd::write(self.file.as_fd(), &bytes[at..count]) {
                    Ok(0) => return Err(Error::new(ErrorKind::Operation, "session output closed")),
                    Ok(written) => at += written,
                    Err(Errno::EINTR) => continue,
                    Err(Errno::EAGAIN) => {
                        let mut descriptors = [PollFd::new(self.file.as_fd(), PollFlags::POLLOUT)];
                        match poll(&mut descriptors, 25u16) {
                            Ok(_) | Err(Errno::EINTR) => {}
                            Err(error) => {
                                return Err(Error::caused_by("wait for session output", error));
                            }
                        }
                    }
                    Err(error) => return Err(Error::caused_by("write session output", error)),
                }
            }
        }
    }
}

impl<F: AsFd> Drop for Writer<F> {
    fn drop(&mut self) {
        let _ = fcntl(self.file.as_fd(), FcntlArg::F_SETFL(self.previous));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_flags_are_restored_when_the_transport_scope_ends() -> Result<()> {
        let (_read, write) = unistd::pipe().map_err(|e| Error::caused_by("test pipe", e))?;
        let previous = fcntl(write.as_fd(), FcntlArg::F_GETFL)
            .map_err(|e| Error::caused_by("read test flags", e))?;
        {
            let _writer = Writer::new(write.as_fd())?;
            let active = fcntl(write.as_fd(), FcntlArg::F_GETFL)
                .map_err(|e| Error::caused_by("read active flags", e))?;
            assert!(OFlag::from_bits_truncate(active).contains(OFlag::O_NONBLOCK));
        }
        assert_eq!(
            fcntl(write.as_fd(), FcntlArg::F_GETFL)
                .map_err(|e| Error::caused_by("read restored flags", e))?,
            previous
        );
        Ok(())
    }
}
