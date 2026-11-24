// Purpose: native 단일 worker와 일반 worker의 입력 배정·backpressure·종료 전달.
// Pattern: Single Owner / Bounded Queue.
// Usage: pipeline이 큐를 소유하고 discovery·worker가 각 endpoint를 사용.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/application/analysis/workers.rs.

use super::events::Work;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crossbeam_channel as channel;
use std::time::Duration;

pub(super) struct Router {
    pub general: channel::Sender<Work>,
    pub native: channel::Sender<Work>,
}
impl Router {
    pub fn send(&self, mut work: Work, cancel: &Cancellation) -> Result<()> {
        loop {
            cancel.check()?;
            let sender = if work.kind.is_native() {
                &self.native
            } else {
                &self.general
            };
            match sender.send_timeout(work, Duration::from_millis(25)) {
                Ok(()) => return Ok(()),
                Err(channel::SendTimeoutError::Timeout(pending)) => work = pending,
                Err(channel::SendTimeoutError::Disconnected(_)) => {
                    return Err(Error::operation(
                        "analysis input owner closed before discovery finished",
                    ));
                }
            }
        }
    }
}

pub(super) struct Inbox {
    general: channel::Receiver<Work>,
    native: channel::Receiver<Work>,
    general_closed: bool,
    native_closed: bool,
    native_owner: bool,
}
impl Inbox {
    pub fn new(general: channel::Receiver<Work>, native: Option<channel::Receiver<Work>>) -> Self {
        let native_owner = native.is_some();
        Self {
            general,
            native: native.unwrap_or_else(channel::never),
            general_closed: false,
            native_closed: !native_owner,
            native_owner,
        }
    }
    pub fn native_owner(&self) -> bool {
        self.native_owner
    }

    pub fn receive(&mut self) -> std::result::Result<Work, channel::RecvTimeoutError> {
        loop {
            if self.general_closed && self.native_closed {
                return Err(channel::RecvTimeoutError::Disconnected);
            }
            let (result, native) = channel::select_biased! {
                recv(self.native) -> item => (item, true),
                recv(self.general) -> item => (item, false),
                default(Duration::from_millis(25)) => return Err(channel::RecvTimeoutError::Timeout),
            };
            match result {
                Ok(work) => return Ok(work),
                Err(_) if native => {
                    self.native = channel::never();
                    self.native_closed = true;
                }
                Err(_) => {
                    self.general = channel::never();
                    self.general_closed = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
