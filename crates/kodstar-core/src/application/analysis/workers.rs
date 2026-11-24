// Purpose: parser worker 수명·실패·취소와 파일별 배치 추출 실행.
// Pattern: Worker.
// Usage: pipeline이 scoped thread로 실행하고 종료를 기다림.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/application/analysis/batches.rs.

use super::events::{Event, Events};
use crate::domain::diagnostics::Error;
use crate::infrastructure::parse::ParseSession;
use crossbeam_channel as channel;

mod extraction;

pub(super) fn run(
    mut receiver: super::routing::Inbox,
    events: Events,
    fact_limit: usize,
    resources: super::resources::Resources,
) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut parser = None;
        loop {
            if events.cancel.is_cancelled() {
                break;
            }
            let work = receiver.receive();
            let work = match work {
                Ok(work) => work,
                Err(channel::RecvTimeoutError::Timeout) => continue,
                Err(channel::RecvTimeoutError::Disconnected) => break,
            };
            let parser = parser.get_or_insert_with(|| {
                let mut parser = ParseSession::new(fact_limit);
                if receiver.native_owner()
                    && let Some(native) = &resources.native
                {
                    parser.configure_native(native.clone());
                }
                parser
            });
            if work.kind.is_native() {
                if !receiver.native_owner() {
                    return Err(Error::operation("native input reached a general worker"));
                }
                let task = work.record.path.to_string_lossy().into_owned();
                events.send(Event::Progress(crate::domain::results::Progress::task(
                    "native",
                    task,
                    Some(format!(
                        "file workspace: {} MiB; worker stack: {} MiB",
                        resources.bytes / (1024 * 1024),
                        super::resources::WORKER_STACK_BYTES / (1024 * 1024)
                    )),
                )))?;
            }
            parser.begin_file(true);
            let result = extraction::run(parser, &work, &events, fact_limit);
            let native = parser.native_report();
            parser.end_file();
            if let Err(error) = result {
                if let Some(issue) = error.input.clone() {
                    events.send(Event::Failed {
                        record: work.record,
                        issue,
                        message: error.to_string(),
                        native,
                    })?;
                } else {
                    events.fail(error);
                    break;
                }
            }
        }
        Ok::<(), Error>(())
    }));
    match outcome {
        Err(_) => events.fail(Error::operation("analysis worker panicked")),
        Ok(Err(error)) => events.fail(error),
        Ok(Ok(())) => {}
    }
}
