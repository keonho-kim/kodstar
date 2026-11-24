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
    pub(super) prefix: Option<crate::domain::facts::append::Prefix>,
}
#[derive(Clone, Copy)]
pub(super) enum InputKind {
    Code(Language),
    Auxiliary,
    Configuration,
}
impl InputKind {
    pub const fn is_native(self) -> bool {
        matches!(self, Self::Code(Language::C | Language::Cpp))
    }
}
pub(super) enum FileChange {
    Replace,
    Body,
    Append(crate::domain::facts::append::Candidate),
}
pub(super) enum Event {
    Progress(crate::domain::results::Progress),
    Remove {
        path: PathBuf,
    },
    Begin {
        record: FileRecord,
        change: FileChange,
        partial: Option<SourceSpan>,
    },
    Facts {
        path: Arc<PathBuf>,
        facts: Vec<(Language, Fact)>,
    },
    End {
        path: Arc<PathBuf>,
        fingerprints: Fingerprints,
        native: Option<Arc<crate::domain::native::Report>>,
        fragments: Vec<usize>,
        partial: Option<SourceSpan>,
    },
    UnchangedFacts {
        record: FileRecord,
        span: SourceSpan,
        scope_ends: Vec<(u32, usize)>,
        native: Option<Arc<crate::domain::native::Report>>,
        fragments: Vec<usize>,
        partial: Option<SourceSpan>,
    },
    Reuse {
        record: FileRecord,
        update: bool,
    },
    Skip {
        record: FileRecord,
        reason: &'static str,
    },
    Failed {
        record: FileRecord,
        issue: crate::domain::diagnostics::InputIssue,
        message: String,
        native: Option<Arc<crate::domain::native::Report>>,
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
            Event::Progress(value) => value.owned_bytes(),
            Event::Remove { path } => path.capacity(),
            Event::Facts { facts, .. } => {
                facts.capacity() * std::mem::size_of::<(Language, Fact)>()
                    + facts
                        .iter()
                        .map(|(_, fact)| fact.owned_bytes() - std::mem::size_of::<Fact>())
                        .sum::<usize>()
            }
            Event::UnchangedFacts {
                record,
                scope_ends,
                fragments,
                ..
            } => {
                record.owned_bytes()
                    + scope_ends.capacity() * std::mem::size_of::<(u32, usize)>()
                    + fragments.capacity() * std::mem::size_of::<usize>()
            }
            Event::Begin { record, change, .. } => {
                record.owned_bytes()
                    + match change {
                        FileChange::Append(a) => {
                            a.names.capacity() * std::mem::size_of::<String>()
                                + a.names.iter().map(String::capacity).sum::<usize>()
                        }
                        _ => 0,
                    }
            }
            Event::Reuse { record, .. } | Event::Skip { record, .. } => record.owned_bytes(),
            Event::Failed {
                record, message, ..
            } => record.owned_bytes() + message.capacity(),
            Event::End { fragments, .. } => fragments.capacity() * std::mem::size_of::<usize>(),
        } + std::mem::size_of::<Message>()
            + match &event {
                Event::End { native, .. }
                | Event::UnchangedFacts { native, .. }
                | Event::Failed { native, .. } => native.as_ref().map_or(0, |r| r.owned_bytes()),
                _ => 0,
            };
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
mod tests;
