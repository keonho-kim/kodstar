// Purpose: 제한된 요청 dispatch·제어 취소·종료와 worker join 소유.
// Pattern: Coordinator.
// Usage: serve --stdio 실행이 세션 전체 수명을 소유.
// Related: crates/kodstar-cli/src/session/protocol.rs, crates/kodstar-cli/src/session/input.rs, crates/kodstar-cli/src/session/output.rs, crates/kodstar-cli/src/session/worker.rs.

mod frame;
mod input;
mod output;
mod protocol;
mod query;
mod transport;
mod worker;
use input::{Frame, Input};
use kodstar_core::{Cancellation, Engine, Error, ErrorKind, Result};
use output::Output;
use protocol::{Id, Operation, Work};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};

type Tokens = Arc<Mutex<HashMap<Id, Cancellation>>>;
struct Job {
    id: Id,
    work: Work,
    cancel: Cancellation,
}
enum End {
    Eof,
    Shutdown,
}

pub(crate) fn run(process: &Cancellation) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|error| Error::caused_by("read session current directory", error))?;
    let engine = Engine::from_environment()?;
    let tokens = Arc::new(Mutex::new(HashMap::new()));
    let (output, transport) = Output::new(process)?;
    let (sender, receiver) = mpsc::sync_channel(2);
    std::thread::scope(|scope| {
        let transport_stop = process.clone();
        let writer = scope.spawn(move || {
            let outcome = transport.run();
            if outcome.is_err() {
                transport_stop.cancel();
            }
            outcome
        });
        let worker_output = output.clone();
        let worker_tokens = Arc::clone(&tokens);
        let worker_stop = process.clone();
        let subscriber = tracing::dispatcher::get_default(Clone::clone);
        let worker = scope.spawn(move || {
            tracing::dispatcher::with_default(&subscriber, || {
                worker::run(engine, receiver, worker_output, worker_tokens, worker_stop)
            })
        });
        let outcome = dispatch(&cwd, &sender, &tokens, &output, process);
        if !matches!(outcome, Ok(End::Eof)) {
            cancel_active(&tokens);
        }
        if outcome.is_err() {
            process.cancel();
        }
        drop(sender);
        let joined = worker.join().unwrap_or_else(|_| {
            process.cancel();
            Err(Error::new(ErrorKind::Operation, "session worker panicked"))
        });
        if matches!(outcome, Ok(End::Shutdown)) {
            output.closing();
        }
        drop(output);
        let written = writer
            .join()
            .unwrap_or_else(|_| Err(Error::new(ErrorKind::Operation, "session writer panicked")));
        let written = match written {
            Err(error) if error.kind != ErrorKind::Cancelled => return Err(error),
            other => other,
        };
        match outcome {
            Err(error) if error.kind != ErrorKind::Cancelled => Err(error),
            Err(error) => {
                joined?;
                Err(error)
            }
            Ok(_) => {
                joined?;
                written
            }
        }
    })
}

fn dispatch(
    cwd: &Path,
    jobs: &mpsc::SyncSender<Job>,
    tokens: &Tokens,
    output: &Output,
    process: &Cancellation,
) -> Result<End> {
    let mut input = Input::new();
    while let Some(frame) = input.next(process)? {
        let frame = match frame {
            Frame::Data(frame) if frame.iter().all(u8::is_ascii_whitespace) => continue,
            Frame::Data(frame) => frame,
            Frame::TooLong => {
                reject(
                    output,
                    None,
                    &Error::argument("session frame exceeds 65536 bytes"),
                )?;
                continue;
            }
        };
        let request = match protocol::decode(&frame, cwd) {
            Ok(request) => request,
            Err(rejected) => {
                reject(output, rejected.id.as_ref(), &rejected.error)?;
                continue;
            }
        };
        match request.operation {
            Operation::Cancel(target) => {
                let token = tokens
                    .lock()
                    .map_err(|_| {
                        Error::new(ErrorKind::Operation, "session request registry poisoned")
                    })?
                    .get(&target)
                    .cloned();
                if let Some(token) = token {
                    token.cancel();
                    output.complete(Some(&request.id), 0)?;
                } else {
                    reject(
                        output,
                        Some(&request.id),
                        &Error::new(ErrorKind::NotFound, "request is not active"),
                    )?;
                }
            }
            Operation::Shutdown => {
                cancel_active(tokens);
                output.complete(Some(&request.id), 0)?;
                return Ok(End::Shutdown);
            }
            Operation::Run(work) => {
                let cancel = crate::signals::cancellation();
                {
                    let mut registry = tokens.lock().map_err(|_| {
                        Error::new(ErrorKind::Operation, "session request registry poisoned")
                    })?;
                    if registry.contains_key(&request.id) {
                        drop(registry);
                        reject(
                            output,
                            Some(&request.id),
                            &Error::argument("request id is already active"),
                        )?;
                        continue;
                    }
                    registry.insert(request.id.clone(), cancel.clone());
                }
                match jobs.try_send(Job {
                    id: request.id.clone(),
                    work: *work,
                    cancel,
                }) {
                    Ok(()) => {}
                    Err(error) => {
                        tokens
                            .lock()
                            .map_err(|_| {
                                Error::new(
                                    ErrorKind::Operation,
                                    "session request registry poisoned",
                                )
                            })?
                            .remove(&request.id);
                        let error = match error {
                            mpsc::TrySendError::Full(_) => Error::new(
                                ErrorKind::Limit,
                                "session queue is full; request was not executed",
                            ),
                            mpsc::TrySendError::Disconnected(_) => {
                                return Err(Error::new(
                                    ErrorKind::Operation,
                                    "session worker closed",
                                ));
                            }
                        };
                        reject(output, Some(&request.id), &error)?;
                    }
                }
            }
        }
    }
    Ok(End::Eof)
}

fn reject(output: &Output, id: Option<&Id>, error: &Error) -> Result<()> {
    output.error(id, error)?;
    output.complete(id, error.kind.exit_code())
}

fn cancel_active(tokens: &Tokens) {
    let registry = tokens
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for token in registry.values() {
        token.cancel();
    }
}

fn check(cancel: &Cancellation) -> Result<()> {
    if cancel.is_cancelled() {
        Err(Error::new(ErrorKind::Cancelled, "session cancelled"))
    } else {
        Ok(())
    }
}
