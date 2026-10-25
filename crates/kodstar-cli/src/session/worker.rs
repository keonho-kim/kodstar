// Purpose: 제한된 세션 작업을 같은 엔진 API로 순차 실행하고 완료 통지.
// Pattern: Worker.
// Usage: 세션 dispatcher가 단일 scoped thread로 소유하고 join.
// Related: crates/kodstar-cli/src/session/mod.rs, crates/kodstar-cli/src/session/output.rs.

use super::{
    Job, Tokens,
    output::{Output, Sink},
};
use kodstar_core::{Cancellation, Engine, Error, ErrorKind, QuerySummary, Result};
use std::sync::mpsc;

pub(super) fn run(
    engine: Engine,
    jobs: mpsc::Receiver<Job>,
    output: Output,
    tokens: Tokens,
    stopped: Cancellation,
) -> Result<()> {
    for job in jobs {
        let mut sink = Sink {
            id: job.id.clone(),
            output: output.clone(),
            cancel: job.cancel.clone(),
        };
        let outcome = if job.cancel.is_cancelled() {
            Err(Error::new(
                ErrorKind::Cancelled,
                "request cancelled before execution",
            ))
        } else {
            execute(&engine, job.work, &job.cancel, &mut sink)
        };
        let code = match outcome {
            Ok(summary) => {
                if summary.complete && !summary.truncated {
                    0
                } else {
                    3
                }
            }
            Err(error) => {
                if let Err(write) = output.error(Some(&job.id), &error) {
                    stopped.cancel();
                    return Err(if write.kind == ErrorKind::Cancelled {
                        error
                    } else {
                        write
                    });
                }
                error.kind.exit_code()
            }
        };
        tokens
            .lock()
            .map_err(|_| Error::new(ErrorKind::Operation, "session request registry poisoned"))?
            .remove(&job.id);
        if let Err(error) = output.complete(Some(&job.id), code) {
            stopped.cancel();
            return Err(error);
        }
    }
    Ok(())
}

fn execute(
    engine: &Engine,
    work: super::protocol::Work,
    cancel: &Cancellation,
    sink: &mut Sink,
) -> Result<QuerySummary> {
    let project = work
        .project
        .ok_or_else(|| Error::argument("project options are required"))?;
    engine.execute(&project, work.action.command()?, cancel, sink)
}
