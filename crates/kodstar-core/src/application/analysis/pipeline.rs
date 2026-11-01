// Purpose: 분석 worker 실행·취소·join과 이벤트 저장.
// Pattern: Coordinator.
// Usage: 분석 transaction 소유자가 호출.
// Related: crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/src/application/analysis/discovery.rs.

use super::budget::ByteBudget;
use super::discovery::Pipeline;
use super::events::{Event, Events, Inbox, Work};
use super::incremental::Resolution;
use crate::application::workspace::SelectedProject;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::results::Progress;
use crate::infrastructure::cache::paths::path_bytes;
use crate::infrastructure::index::{self};
use crossbeam_channel as channel;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

mod start;

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
    let Input {
        database,
        selected,
        full,
        verify,
    } = input;
    let connection = &index::cached::Sql::new(connection)?;
    let mut reuse = super::incremental::ReusePolicy::default();
    let internal = Cancellation::default();
    let (events_tx, events_rx) = channel::bounded(selected.settings.jobs * 4);
    let (work_tx, work_rx) = channel::bounded::<Work>(selected.settings.jobs);
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
                producer.produce(work_tx)
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
        for _ in 0..selected.settings.jobs {
            let receiver = work_rx.clone();
            let events = events.clone();
            let fact_limit = selected.settings.fact_batch_bytes;
            scope.spawn(move || {
                super::workers::run(receiver, events, fact_limit);
            });
        }
        drop(work_rx);
        drop(events);
        let mut writer = None;
        let mut files = HashMap::new();
        let mut changed = false;
        let mut resolution = Resolution::None;
        let mut parsed = 0;
        let mut reused = 0;
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
                match message.event {
                    Event::Remove { path } => {
                        connection.execute("INSERT OR IGNORE INTO deleted_files SELECT id FROM files WHERE path=?1",
                            [path_bytes(&path)])?;
                    }
                    Event::Begin { record, preserve } => {
                        let context = start::file(
                            connection,
                            &record,
                            preserve,
                            full,
                            &mut reuse,
                            &mut resolution,
                            selected.settings.fact_batch_bytes,
                        )?;
                        files.insert(record.path, context);
                        changed = true;
                    }
                    Event::Facts { path, facts } => {
                        let context = files.get_mut(path.as_ref()).ok_or_else(|| {
                            Error::operation("fact arrived outside its file transaction")
                        })?;
                        let writer = match &mut writer {
                            Some(writer) => writer,
                            slot @ None => slot.insert(index::FactWriter::new(connection)?),
                        };
                        for (language, fact) in facts {
                            cancel.check()?;
                            writer.persist(context, language, fact)?;
                        }
                    }
                    Event::End { path, fingerprints } => {
                        let context = files.remove(path.as_ref()).ok_or_else(|| {
                            Error::operation("file completion has no matching extraction")
                        })?;
                        if context.call_reuse {
                            index::body_reuse::finish(connection, context.id)?;
                        }
                        if resolution == Resolution::Selected
                            && !context.preserves_definitions()
                            && !index::invalidation::safe_file(connection, context.id)?
                        {
                            resolution.reset_all(connection)?;
                        }
                        connection.execute(
                            "UPDATE files SET facts_hash=?1,environment_hash=?2 WHERE id=?3",
                            params![
                                &index::hash_bytes(&fingerprints.all)?[..],
                                &index::hash_bytes(&fingerprints.environment)?[..],
                                context.id
                            ],
                        )?;
                        parsed += 1;
                        progress(&Progress {
                            phase: "parse",
                            completed_files: parsed + reused,
                            source_bytes: 0,
                        })?;
                    }
                    Event::UnchangedFacts {
                        record,
                        span,
                        scope_ends,
                    } => {
                        connection.execute(
                            "INSERT OR IGNORE INTO seen_files VALUES(?1)",
                            [path_bytes(&record.path)],
                        )?;
                        record.refresh_unchanged(connection, span, &scope_ends)?;
                        changed = true;
                        parsed += 1;
                        progress(&Progress {
                            phase: "parse",
                            completed_files: parsed + reused,
                            source_bytes: 0,
                        })?;
                    }
                    Event::Reuse { record, update } => {
                        connection.execute(
                            "INSERT OR IGNORE INTO seen_files VALUES(?1)",
                            [path_bytes(&record.path)],
                        )?;
                        if update {
                            record.refresh_metadata(connection)?;
                        }
                        reused += 1;
                    }
                    Event::Skip { record, reason } => {
                        connection.execute(
                            "INSERT OR IGNORE INTO seen_files VALUES(?1)",
                            [path_bytes(&record.path)],
                        )?;
                        if index::inert_inputs::unchanged_kind(connection, &record)? {
                            record.refresh_metadata(connection)?;
                            changed = true;
                            return Ok(());
                        }
                        resolution.reset_all(connection)?;
                        let id = record.persist(connection)?;
                        index::clear_file(connection, id)?;
                        connection.execute(
                            "INSERT INTO diagnostics(file_id,code,message) VALUES(?1,?2,?3)",
                            params![id, reason, reason],
                        )?;
                        changed = true;
                    }
                }
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
        } else if !files.is_empty() {
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
            Ok((changed, parsed, reused + inventory.retained, resolution))
        }
    })
}
