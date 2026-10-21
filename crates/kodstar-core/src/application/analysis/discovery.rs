// Purpose: 파일 발견·변경 판정과 제한된 입력 전달.
// Pattern: Producer.
// Usage: pipeline이 소유한 scanner thread에서 호출.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/application/analysis/events.rs.

use super::budget::ByteBudget;
use super::events::{Event, Events, InputKind, Work};
use super::module_identity::module_identity;
use crate::application::workspace::SelectedProject;
use crate::domain::diagnostics::{Error, Result};
use crate::infrastructure::cache::CacheStore;
use crate::infrastructure::index::{self, FileRecord};
use crate::infrastructure::{formats, scan};
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

pub(super) struct Pipeline<'a> {
    pub(super) selected: &'a SelectedProject,
    pub(super) database: &'a Path,
    pub(super) store: &'a CacheStore,
    pub(super) full: bool,
    pub(super) verify: bool,
    pub(super) inputs: Arc<ByteBudget>,
    pub(super) events: Events,
}

impl Pipeline<'_> {
    pub(super) fn produce(&self, sender: mpsc::SyncSender<Work>) -> Result<()> {
        let reader = index::open(
            self.database,
            &self.selected.root,
            false,
            self.selected.settings.db_cache_bytes,
        )?;
        index::limit_reads(&reader, self.selected.settings.memory_budget)?;
        index::observe_cancellation(&reader, &self.events.cancel)?;
        let overrides: Vec<_> = self
            .selected
            .settings
            .language_overrides
            .iter()
            .map(|(pattern, language)| {
                globset::Glob::new(pattern).map(|glob| (glob.compile_matcher(), *language))
            })
            .collect::<std::result::Result<_, _>>()
            .map_err(|error| Error::caused_by("compile language overrides", error))?;
        let mut previous_query = index::PreviousFiles::new(&reader)?;
        scan::walk(
            &self.selected.root,
            &self.store.home,
            &self.selected.settings.exclude,
            &overrides,
            &self.events.cancel,
            &mut |mut entry| {
                let (modified, changed) = scan::metadata_key(&entry.metadata)?;
                let previous = previous_query.get(&entry.relative)?;
                if entry.detect_prefix {
                    entry.language = if !self.full
                        && !self.verify
                        && previous.as_ref().is_some_and(|old| {
                            old.modified == modified
                                && old.changed == changed
                                && old.size == entry.metadata.len()
                        }) {
                        previous.as_ref().and_then(|old| old.language.parse().ok())
                    } else {
                        scan::detect_from_prefix(&entry)?
                    };
                }
                let configuration = entry.language.is_none()
                    && (formats::is_configuration(&entry.relative)
                        || previous.as_ref().is_some_and(|old| old.configuration));
                let auxiliary = entry.language.is_none() && (entry.auxiliary || configuration);
                let language =
                    entry
                        .language
                        .map(|language| language.as_str())
                        .unwrap_or(if auxiliary {
                            "auxiliary"
                        } else {
                            "unsupported"
                        });
                let (package, module) = module_identity(
                    &self.selected.root,
                    &entry.relative,
                    entry.language.or_else(|| {
                        auxiliary.then(|| {
                            if configuration {
                                crate::domain::languages::Language::TypeScript
                            } else {
                                formats::auxiliary_language(&entry.relative)
                            }
                        })
                    }),
                );
                let mut record = FileRecord {
                    path: entry.relative.clone(),
                    language: language.into(),
                    module,
                    package,
                    content_hash: previous
                        .as_ref()
                        .map(|record| record.hash.clone())
                        .unwrap_or_default(),
                    size: entry.metadata.len(),
                    modified_ns: modified,
                    changed_ns: changed,
                    status: if auxiliary { "auxiliary" } else { "analyzed" },
                };
                if entry.language.is_none() && !auxiliary {
                    record.status = "unsupported";
                    if !self.full
                        && previous
                            .as_ref()
                            .is_some_and(|old| old.same_metadata(&record))
                    {
                        return self.events.send(Event::Reuse {
                            record,
                            update: false,
                        });
                    }
                    return self.events.send(Event::Skip {
                        record,
                        reason: "unsupported_input",
                    });
                }
                if entry.metadata.len() > self.selected.settings.max_file_bytes as u64 {
                    record.status = "skipped";
                    if !self.full
                        && previous
                            .as_ref()
                            .is_some_and(|old| old.same_metadata(&record))
                    {
                        return self.events.send(Event::Reuse {
                            record,
                            update: false,
                        });
                    }
                    return self.events.send(Event::Skip {
                        record,
                        reason: "file_size_limit",
                    });
                }
                if !self.full
                    && !self.verify
                    && previous
                        .as_ref()
                        .is_some_and(|old| old.same_metadata(&record))
                {
                    return self.events.send(Event::Reuse {
                        record,
                        update: false,
                    });
                }
                let input = self.inputs.reserve(
                    self.selected.settings.input_per_worker()?,
                    &self.events.cancel,
                )?;
                let (source, metadata) =
                    scan::read_stable(&entry, self.selected.settings.max_file_bytes)?;
                record.size = metadata.len();
                (record.modified_ns, record.changed_ns) = scan::metadata_key(&metadata)?;
                if source.len() > self.selected.settings.max_file_bytes {
                    record.status = "skipped";
                    return self.events.send(Event::Skip {
                        record,
                        reason: "file_size_limit",
                    });
                }
                record.content_hash = blake3::hash(&source).to_hex().to_string();
                if !self.full
                    && previous.as_ref().is_some_and(|old| {
                        old.hash == record.content_hash && old.same_context(&record)
                    })
                {
                    return self.events.send(Event::Reuse {
                        record,
                        update: true,
                    });
                }
                let previous = previous
                    .filter(|old| {
                        !self.full && old.same_context(&record) && !old.fingerprints.all.is_empty()
                    })
                    .map(|old| old.fingerprints);
                let mut work = Work {
                    record,
                    kind: entry
                        .language
                        .map(InputKind::Code)
                        .unwrap_or(if configuration {
                            InputKind::Configuration
                        } else {
                            InputKind::Auxiliary
                        }),
                    source,
                    previous,
                    _input: input,
                };
                loop {
                    self.events.cancel.check()?;
                    match sender.try_send(work) {
                        Ok(()) => return Ok(()),
                        Err(mpsc::TrySendError::Full(pending)) => {
                            work = pending;
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(mpsc::TrySendError::Disconnected(_)) => {
                            return Err(Error::operation(
                                "analysis workers terminated before scan completed",
                            ));
                        }
                    }
                }
            },
        )
    }
}
