// Purpose: 분석 worker 실행·취소·join과 이벤트 저장.
// Pattern: Coordinator.
// Usage: 분석 transaction 소유자가 호출.
// Related: crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/src/application/analysis/discovery.rs.

use super::budget::ByteBudget;
use super::discovery::Pipeline;
use super::events::{Events, Inbox, Work};
use super::incremental::Resolution;
use crate::application::workspace::SelectedProject;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::results::Progress;
use crate::infrastructure::index::{self};
use crossbeam_channel as channel;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

mod start;
mod writer;

pub(super) struct Input<'a> {
    pub(super) database: &'a Path,
    pub(super) selected: &'a SelectedProject,
    pub(super) full: bool,
    pub(super) verify: bool,
}

pub(super) fn run(
    input: Input<'_>,
    connection: &Connection,
    cancel: &Cancellation,
    progress: &mut dyn FnMut(&Progress) -> Result<()>,
) -> Result<(bool, u64, u64, Resolution)> {
    let _span = tracing::info_span!("extract", jobs = input.selected.settings.jobs).entered();
    let Input {
        database,
        selected,
        full,
        verify,
    } = input;
    let connection = &index::cached::Sql::new(connection)?;
    let internal = Cancellation::default();
    let (events_tx, events_rx) = channel::bounded(selected.settings.jobs * 4);
    let (work_tx, work_rx) = channel::bounded::<Work>(selected.settings.jobs);
    let (native_tx, native_rx) = channel::bounded::<Work>(1);
    let inputs = ByteBudget::new(
        selected
            .settings
            .input_per_worker()?
            .checked_mul(selected.settings.jobs)
            .ok_or_else(|| Error::argument("input budget overflow"))?,
    )?;
    let worker_failure = Arc::new(Mutex::new(None));
    let events = Events {
        sender: events_tx,
        bytes: ByteBudget::new(selected.settings.queue_bytes)?,
        cancel: internal.clone(),
        failure: Arc::clone(&worker_failure),
    };
    let resources = super::resources::Resources::new(selected)?;
    progress(&Progress::phase("parse"))?;
    thread::scope(|scope| {
        let messages = Inbox {
            receiver: events_rx,
            cancel: internal.clone(),
        };
        let producer = Pipeline {
            selected,
            database,
            full,
            verify,
            inputs,
            events: events.clone(),
        };
        let discovery = scope.spawn(move || {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                producer.produce(super::routing::Router {
                    general: work_tx,
                    native: native_tx,
                })
            })) {
                Ok(Ok(inventory)) => Some(inventory),
                Ok(Err(error)) => {
                    producer.events.fail(error);
                    None
                }
                Err(_) => {
                    producer
                        .events
                        .fail(Error::operation("analysis scanner panicked"));
                    None
                }
            }
        });
        for worker in 0..selected.settings.jobs {
            let receiver = super::routing::Inbox::new(
                work_rx.clone(),
                (worker == 0).then(|| native_rx.clone()),
            );
            let events = events.clone();
            let fact_limit = selected.settings.fact_batch_bytes;
            let resources = resources.clone();
            thread::Builder::new()
                .name(
                    if worker == 0 {
                        "kodstar-native"
                    } else {
                        "kodstar-analysis"
                    }
                    .into(),
                )
                .stack_size(if worker == 0 {
                    super::resources::WORKER_STACK_BYTES
                } else {
                    super::resources::GENERAL_STACK_BYTES
                })
                .spawn_scoped(scope, move || {
                    super::workers::run(receiver, events, fact_limit, resources);
                })
                .map_err(|error| Error::caused_by("start analysis worker", error))?;
        }
        drop(work_rx);
        drop(native_rx);
        drop(events);
        let mut writer = writer::Writer::new(connection, full, selected.settings.fact_batch_bytes);
        let mut failure = None;
        loop {
            if cancel.is_cancelled() && failure.is_none() {
                internal.cancel();
                failure = Some(Error::new(
                    crate::domain::diagnostics::ErrorKind::Cancelled,
                    "operation cancelled",
                ));
            }
            let message = match messages.receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(message) => message,
                Err(channel::RecvTimeoutError::Timeout) => continue,
                Err(channel::RecvTimeoutError::Disconnected) => break,
            };
            let result: Result<()> = (|| {
                if failure.is_some() {
                    return Ok(());
                }
                writer.accept(message.event, cancel, progress)?;
                Ok(())
            })();
            if let Err(error) = result {
                if failure.is_none() {
                    failure = Some(error);
                }
                internal.cancel();
            }
        }
        let inventory = discovery
            .join()
            .map_err(|_| Error::operation("analysis scanner panicked"))?;
        let worker_error = worker_failure
            .lock()
            .map_err(|_| Error::operation("analysis failure state lock poisoned"))?
            .take();
        if let Some(error) = failure.or(worker_error) {
            Err(error)
        } else if writer.unfinished() {
            Err(Error::operation("analysis ended with unfinished files"))
        } else {
            let inventory =
                inventory.ok_or_else(|| Error::operation("analysis inventory did not complete"))?;
            if !inventory.partial {
                connection.execute_batch(
                    "INSERT OR IGNORE INTO deleted_files
                    SELECT id FROM files WHERE path NOT IN (SELECT path FROM seen_files);",
                )?;
            }
            Ok((
                writer.changed,
                writer.parsed,
                writer.reused + inventory.retained,
                writer.resolution,
            ))
        }
    })
}
