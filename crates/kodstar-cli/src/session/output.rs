// Purpose: 요청 ID를 보존하는 세션 결과·진행·오류·완료 이벤트 전송.
// Pattern: Output Adapter.
// Usage: dispatcher와 한 worker가 frame 단위로 stdout을 공유.
// Related: crates/kodstar-cli/src/session/mod.rs, crates/kodstar-cli/src/session/worker.rs.

use super::protocol::Id;
use kodstar_core::{
    Error, ErrorKind, Progress, QueryMetadata, QueryRow, QuerySink, QuerySummary, Result,
};
use serde::Serialize;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

struct State {
    writer: io::BufWriter<io::Stdout>,
    failed: bool,
}
#[derive(Clone)]
pub(super) struct Output {
    state: Arc<Mutex<State>>,
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
    pub(super) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                writer: io::BufWriter::with_capacity(65536, io::stdout()),
                failed: false,
            })),
        }
    }
    fn send(&self, value: &impl Serialize) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorKind::Operation, "session output lock poisoned"))?;
        if state.failed {
            return Err(Error::new(
                ErrorKind::Cancelled,
                "session output has closed",
            ));
        }
        let outcome = (|| {
            serde_json::to_writer(&mut state.writer, value).map_err(io::Error::other)?;
            state.writer.write_all(b"\n")?;
            state.writer.flush()
        })();
        if let Err(error) = outcome {
            state.failed = true;
            return Err(Error::caused_by("write session output", error));
        }
        Ok(())
    }
    pub(super) fn error(&self, id: Option<&Id>, error: &Error) -> Result<()> {
        self.send(&Failure {
            protocol_version: 1,
            id,
            kind: "error",
            code: error.kind.exit_code(),
            message: error.to_string(),
        })
    }
    pub(super) fn complete(&self, id: Option<&Id>, exit_code: u8) -> Result<()> {
        self.send(&Complete {
            protocol_version: 1,
            id,
            kind: "complete",
            exit_code,
        })
    }
    fn event(&self, id: &Id, kind: &'static str, data: &impl Serialize) -> Result<()> {
        self.send(&Event {
            protocol_version: 1,
            id: Some(id),
            kind,
            data,
        })
    }
}

pub(super) struct Sink {
    pub id: Id,
    pub output: Output,
}
impl QuerySink for Sink {
    fn begin(&mut self, value: &QueryMetadata) -> Result<()> {
        self.output.event(&self.id, "metadata", value)
    }
    fn row(&mut self, value: &QueryRow) -> Result<()> {
        self.output.event(&self.id, "result", value)
    }
    fn finish(&mut self, value: &QuerySummary) -> Result<()> {
        self.output.event(&self.id, "summary", value)
    }
    fn progress(&mut self, value: &Progress) -> Result<()> {
        self.output.event(&self.id, "progress", value)
    }
}
