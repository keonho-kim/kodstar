// Purpose: 파일 발견·변경 판정과 제한된 입력 전달.
// Pattern: Producer.
// Usage: pipeline이 소유한 scanner thread에서 호출.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/application/analysis/events.rs.

use super::budget::ByteBudget;
use super::events::{Event, Events, InputKind, Work};
use super::module_identity::{module_identity, package_path};
use crate::application::workspace::SelectedProject;
use crate::domain::diagnostics::{Error, Result};
use crate::infrastructure::index::{self, FileRecord};
use crate::infrastructure::{formats, history::inputs, languages};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

pub(super) struct Pipeline<'a> {
    pub(super) selected: &'a SelectedProject,
    pub(super) database: &'a Path,
    pub(super) full: bool,
    pub(super) verify: bool,
    pub(super) inputs: Arc<ByteBudget>,
    pub(super) events: Events,
}

pub(super) struct Inventory {
    pub partial: bool,
    pub retained: u64,
}

impl Pipeline<'_> {
    pub(super) fn produce(&self, sender: super::routing::Router) -> Result<Inventory> {
        let reader = index::open(
            self.database,
            &self.selected.root,
            false,
            self.selected.settings.db_cache_bytes,
        )?;
        index::limit_reads(&reader, self.selected.settings.memory_budget)?;
        index::observe_cancellation(&reader, &self.events.cancel)?;
        let selection = super::incremental::inputs::prepare(
            self.selected,
            &reader,
            self.full,
            self.verify,
            &self.events.cancel,
        )?;
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
        let mut touched = 0u64;
        let mut emit = |mut entry: inputs::Entry| {
            let previous = previous_query.get(&entry.relative)?;
            let native_changed = !self.full
                && previous.is_some()
                && matches!(
                    entry.language,
                    Some(
                        crate::domain::languages::Language::C
                            | crate::domain::languages::Language::Cpp
                    )
                )
                && super::native_inputs::changed(
                    &reader,
                    &selection.connection,
                    &entry.relative,
                    &self.events.cancel,
                )?;
            touched += u64::from(previous.is_some());
            let mut detected_input = None;
            if entry.detect_prefix {
                entry.language = if !self.full
                    && !self.verify
                    && previous.as_ref().is_some_and(|old| {
                        old.hash == entry.content_hash() && old.size == entry.size()
                    }) {
                    previous.as_ref().and_then(|old| old.language.parse().ok())
                } else if entry
                    .relative
                    .extension()
                    .is_some_and(|extension| extension == "h")
                    && entry.size() <= self.selected.settings.max_file_bytes as u64
                {
                    let input = self.inputs.reserve(
                        self.selected.settings.input_per_worker()?,
                        &self.events.cancel,
                    )?;
                    let source =
                        entry.read(self.selected.settings.max_file_bytes, &self.events.cancel)?;
                    let language = languages::detect(&entry.relative, &source);
                    detected_input = Some((input, source));
                    language
                } else {
                    entry.detect_from_prefix()?
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
            let identity_language = entry.language.or_else(|| {
                auxiliary.then(|| {
                    if configuration {
                        crate::domain::languages::Language::TypeScript
                    } else {
                        formats::auxiliary_language(&entry.relative)
                    }
                })
            });
            let package_path = package_path(&entry, identity_language)?;
            let (package, module) = module_identity(
                &self.selected.root,
                &entry.relative,
                identity_language,
                &package_path,
            );
            let mut record = FileRecord {
                path: entry.relative.clone(),
                language: language.into(),
                module,
                package,
                content_hash: entry.content_hash(),
                size: entry.size(),
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
            if entry.size() > self.selected.settings.max_file_bytes as u64 {
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
                && !native_changed
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
            let (input, source) = match detected_input {
                Some(input) => input,
                None => {
                    let input = self.inputs.reserve(
                        self.selected.settings.input_per_worker()?,
                        &self.events.cancel,
                    )?;
                    let source =
                        entry.read(self.selected.settings.max_file_bytes, &self.events.cancel)?;
                    (input, source)
                }
            };
            record.size = source.len() as u64;
            if source.len() > self.selected.settings.max_file_bytes {
                record.status = "skipped";
                return self.events.send(Event::Skip {
                    record,
                    reason: "file_size_limit",
                });
            }
            record.content_hash = entry.content_hash();
            if !self.full
                && !native_changed
                && previous
                    .as_ref()
                    .is_some_and(|old| old.hash == record.content_hash && old.same_context(&record))
            {
                return self.events.send(Event::Reuse {
                    record,
                    update: true,
                });
            }
            let prefix = previous
                .as_ref()
                .filter(|old| !self.full && old.same_context(&record))
                .and_then(|old| {
                    usize::try_from(old.size).ok().map(|bytes| {
                        crate::domain::facts::append::Prefix {
                            bytes,
                            hash: old.hash,
                        }
                    })
                });
            let previous = previous
                .filter(|old| !self.full && old.same_context(&record))
                .and_then(|old| old.fingerprints);
            let work = Work {
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
                prefix,
                _input: input,
            };
            sender.send(work, &self.events.cancel)
        };
        inputs::walk(
            Rc::clone(&selection.connection),
            selection.partial,
            &self.selected.settings.exclude,
            &overrides,
            &self.events.cancel,
            &mut emit,
        )?;
        drop(emit);
        if selection.partial {
            let mut q = selection.connection.prepare(
                "SELECT path FROM snapshot_changes
                WHERE before_blob IS NOT NULL AND before_attributes&512=0
                AND (after_blob IS NULL OR after_attributes&512<>0)",
            )?;
            let mut rows = q.query([])?;
            while let Some(row) = rows.next()? {
                self.events.cancel.check()?;
                let path = crate::infrastructure::cache::paths::path_from_bytes(
                    &row.get::<_, Vec<u8>>(0)?,
                )?;
                if previous_query.get(&path)?.is_some() {
                    touched += 1;
                    self.events.send(Event::Remove { path })?;
                }
            }
        }
        Ok(Inventory {
            partial: selection.partial,
            retained: if selection.partial {
                selection
                    .previous_files
                    .checked_sub(touched)
                    .ok_or_else(|| {
                        Error::operation("changed input count exceeds the prior analysis")
                    })?
            } else {
                0
            },
        })
    }
}
