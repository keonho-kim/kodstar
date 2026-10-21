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
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

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
    pub(super) receiver: mpsc::Receiver<Message>,
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
    pub(super) sender: mpsc::SyncSender<Message>,
    pub(super) bytes: Arc<ByteBudget>,
    pub(super) cancel: Cancellation,
    pub(super) failure: Arc<Mutex<Option<Error>>>,
}
impl Events {
    pub(super) fn send(&self, event: Event) -> Result<()> {
        let size = match &event {
            Event::Facts { facts, .. } => {
                facts.capacity() * std::mem::size_of::<(Language, Fact)>()
                    + facts
                        .iter()
                        .map(|(_, fact)| fact.owned_bytes() - std::mem::size_of::<Fact>())
                        .sum::<usize>()
            }
            Event::Begin { record, .. }
            | Event::UnchangedFacts { record, .. }
            | Event::Reuse { record, .. }
            | Event::Skip { record, .. } => record.owned_bytes(),
            Event::End { fingerprints, .. } => {
                fingerprints.all.capacity() + fingerprints.environment.capacity()
            }
        } + std::mem::size_of::<Message>();
        let permit = Some(self.bytes.reserve(size, &self.cancel)?);
        let message = Message {
            event,
            _bytes: permit,
        };
        self.cancel.check()?;
        self.sender
            .send(message)
            .map_err(|_| Error::operation("analysis writer closed before its workers finished"))
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
    fn closing_the_writer_releases_queued_and_waiting_producers() -> Result<()> {
        let (sender, receiver) = mpsc::sync_channel(1);
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
        let (started, ready) = mpsc::channel();
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
