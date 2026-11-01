// Purpose: 입력·fact 이벤트의 예산과 최초 실패 전달.
// Pattern: Bounded Channel.
// Usage: discovery·worker가 보내고 pipeline이 소비.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/application/analysis/budget.rs.

use super::budget::{ByteBudget, Permit};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Fact, Fingerprints};
use crate::domain::languages::Language;
use crate::domain::source::SourceSpan;
use crate::infrastructure::index::FileRecord;
use crossbeam_channel as channel;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(super) struct Work {
    pub(super) record: FileRecord,
    pub(super) kind: InputKind,
    pub(super) source: Vec<u8>,
    pub(super) _input: Permit,
    pub(super) previous: Option<Fingerprints>,
}
#[derive(Clone, Copy)]
pub(super) enum InputKind {
    Code(Language),
    Auxiliary,
    Configuration,
}
pub(super) enum Event {
    Remove {
        path: PathBuf,
    },
    Begin {
        record: FileRecord,
        preserve: bool,
    },
    Facts {
        path: Arc<PathBuf>,
        facts: Vec<(Language, Fact)>,
    },
    End {
        path: Arc<PathBuf>,
        fingerprints: Fingerprints,
    },
    UnchangedFacts {
        record: FileRecord,
        span: SourceSpan,
        scope_ends: Vec<(u32, usize)>,
    },
    Reuse {
        record: FileRecord,
        update: bool,
    },
    Skip {
        record: FileRecord,
        reason: &'static str,
    },
}
pub(super) struct Message {
    pub(super) event: Event,
    pub(super) _bytes: Option<Permit>,
}

pub(super) struct Inbox {
    pub(super) receiver: channel::Receiver<Message>,
    pub(super) cancel: Cancellation,
}
impl Drop for Inbox {
    fn drop(&mut self) {
        // The receiver field drops immediately afterward, waking blocked senders before scoped join.
        self.cancel.cancel();
    }
}

#[derive(Clone)]
pub(super) struct Events {
    pub(super) sender: channel::Sender<Message>,
    pub(super) bytes: Arc<ByteBudget>,
    pub(super) cancel: Cancellation,
    pub(super) failure: Arc<Mutex<Option<Error>>>,
}
impl Events {
    pub(super) fn send(&self, event: Event) -> Result<()> {
        let size = match &event {
            Event::Remove { path } => path.capacity(),
            Event::Facts { facts, .. } => {
                facts.capacity() * std::mem::size_of::<(Language, Fact)>()
                    + facts
                        .iter()
                        .map(|(_, fact)| fact.owned_bytes() - std::mem::size_of::<Fact>())
                        .sum::<usize>()
            }
            Event::UnchangedFacts {
                record, scope_ends, ..
            } => record.owned_bytes() + scope_ends.capacity() * std::mem::size_of::<(u32, usize)>(),
            Event::Begin { record, .. }
            | Event::Reuse { record, .. }
            | Event::Skip { record, .. } => record.owned_bytes(),
            Event::End { fingerprints, .. } => {
                fingerprints.all.capacity() + fingerprints.environment.capacity()
            }
        } + std::mem::size_of::<Message>();
        let permit = Some(self.bytes.reserve(size, &self.cancel)?);
        let mut message = Message {
            event,
            _bytes: permit,
        };
        loop {
            self.cancel.check()?;
            match self.sender.send_timeout(message, Duration::from_millis(25)) {
                Ok(()) => return Ok(()),
                Err(channel::SendTimeoutError::Timeout(pending)) => message = pending,
                Err(channel::SendTimeoutError::Disconnected(_)) => {
                    return Err(Error::operation(
                        "analysis writer closed before its workers finished",
                    ));
                }
            }
        }
    }
    pub(super) fn fail(&self, error: Error) {
        if let Ok(mut failure) = self.failure.lock()
            && failure.is_none()
        {
            *failure = Some(error);
        }
        self.cancel.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn cancellation_releases_a_full_queue_without_waiting_for_the_receiver() -> Result<()> {
        let (sender, receiver) = channel::bounded(1);
        let bytes = ByteBudget::new(4096)?;
        let cancel = Cancellation::default();
        let events = Events {
            sender,
            bytes: Arc::clone(&bytes),
            cancel: cancel.clone(),
            failure: Arc::default(),
        };
        let event = || Event::End {
            path: Arc::default(),
            fingerprints: Fingerprints {
                all: String::new(),
                environment: String::new(),
            },
        };
        events.send(event())?;
        let (done, finished) = channel::bounded(1);
        let worker = thread::spawn(move || {
            let _ = done.send(events.send(event()));
        });
        cancel.cancel();
        let result = finished.recv_timeout(Duration::from_secs(2));
        drop(receiver);
        worker
            .join()
            .map_err(|_| Error::operation("test producer panicked"))?;
        let error = result
            .map_err(|e| Error::caused_by("cancelled producer did not stop", e))?
            .expect_err("cancelled send succeeded");
        assert!(matches!(
            error.kind,
            crate::domain::diagnostics::ErrorKind::Cancelled
        ));
        let _all = bytes.reserve(4096, &Cancellation::default())?;
        Ok(())
    }

    #[test]
    fn closing_the_writer_releases_queued_and_waiting_producers() -> Result<()> {
        let (sender, receiver) = channel::bounded(1);
        let cancel = Cancellation::default();
        let bytes = ByteBudget::new(4096)?;
        let events = Events {
            sender,
            bytes: Arc::clone(&bytes),
            cancel: cancel.clone(),
            failure: Arc::default(),
        };
        let inbox = Inbox {
            receiver,
            cancel: cancel.clone(),
        };
        let (started, ready) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            events.send(Event::End {
                path: Arc::default(),
                fingerprints: Fingerprints {
                    all: String::new(),
                    environment: String::new(),
                },
            })?;
            started
                .send(())
                .map_err(|error| Error::caused_by("test producer ready", error))?;
            events.send(Event::End {
                path: Arc::default(),
                fingerprints: Fingerprints {
                    all: String::new(),
                    environment: String::new(),
                },
            })
        });
        ready
            .recv()
            .map_err(|error| Error::caused_by("wait for test producer", error))?;
        drop(inbox);
        let result = worker
            .join()
            .map_err(|_| Error::operation("test producer panicked"))?;
        assert!(result.is_err());
        assert!(cancel.is_cancelled());
        let _all = bytes.reserve(4096, &Cancellation::default())?;
        Ok(())
    }
}
