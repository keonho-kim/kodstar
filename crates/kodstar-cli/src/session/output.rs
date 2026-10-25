// Purpose: 요청 ID를 보존하는 세션 결과·진행·오류·완료 이벤트 전송.
// Pattern: Output Adapter.
// Usage: dispatcher·worker가 프레임을 queue로 보내고 transport가 stdout을 소유.
// Related: crates/kodstar-cli/src/session/mod.rs, crates/kodstar-cli/src/session/worker.rs, crates/kodstar-cli/src/session/transport.rs.

use super::protocol::Id;
use super::{frame::Frame, transport::Transport};
use kodstar_core::{
    Cancellation, Error, ErrorKind, Progress, QueryMetadata, QueryRow, QuerySink, QuerySummary,
    Result,
};
use serde::Serialize;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

struct State {
    sender: mpsc::SyncSender<Queued>,
    slots: Arc<Budget>,
    cancel: Cancellation,
    closing: Arc<AtomicBool>,
}
#[derive(Default)]
struct Slots {
    data: usize,
    control: usize,
}
#[derive(Default)]
struct Budget {
    slots: Mutex<Slots>,
    ready: Condvar,
}
struct Permit {
    slots: Arc<Budget>,
    data: bool,
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut slots = self.slots.slots.lock().unwrap_or_else(|p| p.into_inner());
        if self.data {
            slots.data -= 1;
        } else {
            slots.control -= 1;
        }
        self.slots.ready.notify_all();
    }
}
#[derive(Clone)]
pub(super) struct Output {
    state: Arc<State>,
}
pub(super) struct Queued {
    pub frame: Frame,
    _slot: Permit,
}

#[derive(Serialize)]
struct Event<'a, T> {
    protocol_version: u32,
    id: Option<&'a Id>,
    #[serde(rename = "type")]
    kind: &'static str,
    data: &'a T,
}
#[derive(Serialize)]
struct Complete<'a> {
    protocol_version: u32,
    id: Option<&'a Id>,
    #[serde(rename = "type")]
    kind: &'static str,
    exit_code: u8,
}
#[derive(Serialize)]
struct Failure<'a> {
    protocol_version: u32,
    id: Option<&'a Id>,
    #[serde(rename = "type")]
    kind: &'static str,
    code: u8,
    message: String,
}

impl Output {
    pub(super) fn new(cancel: &Cancellation) -> Result<(Self, Transport)> {
        let (sender, receiver) = mpsc::sync_channel(10);
        let closing = Arc::new(AtomicBool::new(false));
        let transport = Transport::new(receiver, cancel.clone(), Arc::clone(&closing))?;
        Ok((
            Self {
                state: Arc::new(State {
                    sender,
                    slots: Arc::default(),
                    cancel: cancel.clone(),
                    closing,
                }),
            },
            transport,
        ))
    }

    pub(super) fn closing(&self) {
        self.state.closing.store(true, Ordering::Release);
    }

    fn send(&self, value: &impl Serialize, request: Option<&Cancellation>) -> Result<()> {
        let frame =
            Frame::encode(value).map_err(|e| Error::caused_by("encode session frame", e))?;
        loop {
            super::check(&self.state.cancel)?;
            if let Some(cancel) = request {
                super::check(cancel)?;
            }
            let mut slots =
                self.state.slots.slots.lock().map_err(|_| {
                    Error::new(ErrorKind::Operation, "session output budget poisoned")
                })?;
            let (used, limit) = if request.is_some() {
                (&mut slots.data, 2)
            } else {
                (&mut slots.control, 8)
            };
            if *used < limit {
                *used += 1;
                drop(slots);
                let permit = Permit {
                    slots: Arc::clone(&self.state.slots),
                    data: request.is_some(),
                };
                return self
                    .state
                    .sender
                    .try_send(Queued {
                        frame,
                        _slot: permit,
                    })
                    .map_err(|error| match error {
                        mpsc::TrySendError::Disconnected(_) => {
                            Error::new(ErrorKind::Cancelled, "session output has closed")
                        }
                        mpsc::TrySendError::Full(_) => Error::new(
                            ErrorKind::Operation,
                            "session output exceeded its reserved slots",
                        ),
                    });
            }
            if request.is_none() {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "session control output is full",
                ));
            }
            let _wait = self
                .state
                .slots
                .ready
                .wait_timeout(slots, Duration::from_millis(25))
                .map_err(|_| Error::new(ErrorKind::Operation, "session output budget poisoned"))?;
        }
    }
    pub(super) fn error(&self, id: Option<&Id>, error: &Error) -> Result<()> {
        self.send(
            &Failure {
                protocol_version: 2,
                id,
                kind: "error",
                code: error.kind.exit_code(),
                message: error.to_string(),
            },
            None,
        )
    }
    pub(super) fn complete(&self, id: Option<&Id>, exit_code: u8) -> Result<()> {
        self.send(
            &Complete {
                protocol_version: 2,
                id,
                kind: "complete",
                exit_code,
            },
            None,
        )
    }
    fn event(
        &self,
        id: &Id,
        kind: &'static str,
        data: &impl Serialize,
        cancel: &Cancellation,
    ) -> Result<()> {
        self.send(
            &Event {
                protocol_version: 2,
                id: Some(id),
                kind,
                data,
            },
            Some(cancel),
        )
    }
}

pub(super) struct Sink {
    pub id: Id,
    pub output: Output,
    pub cancel: Cancellation,
}
impl QuerySink for Sink {
    fn begin(&mut self, value: &QueryMetadata) -> Result<()> {
        self.output.event(&self.id, "metadata", value, &self.cancel)
    }
    fn row(&mut self, value: &QueryRow) -> Result<()> {
        self.output.event(&self.id, "result", value, &self.cancel)
    }
    fn finish(&mut self, value: &QuerySummary) -> Result<()> {
        self.output.event(&self.id, "summary", value, &self.cancel)
    }
    fn progress(&mut self, value: &Progress) -> Result<()> {
        self.output.event(&self.id, "progress", value, &self.cancel)
    }
}
