// Purpose: 파일 이벤트 저장·본문 재사용과 분석 진행의 단일 writer 상태 소유.
// Pattern: Single Writer.
// Usage: pipeline 조정자가 channel 이벤트를 순서대로 전달.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/application/analysis/pipeline/start.rs.

use super::super::{
    events::Event,
    incremental::{Resolution, ReusePolicy},
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    results::Progress,
};
use crate::infrastructure::{
    cache::paths::path_bytes,
    index::{self, cached::Sql},
};
use rusqlite::params;
use std::{collections::HashMap, path::PathBuf};

pub(super) struct Writer<'conn, 'operation> {
    connection: &'operation Sql<'conn>,
    full: bool,
    fact_budget: usize,
    reuse: ReusePolicy,
    files: HashMap<PathBuf, index::FileFacts>,
    writer: Option<index::FactWriter<'operation>>,
    pub changed: bool,
    pub resolution: Resolution,
    pub parsed: u64,
    pub reused: u64,
}
impl<'conn, 'operation> Writer<'conn, 'operation> {
    pub fn new(connection: &'operation Sql<'conn>, full: bool, fact_budget: usize) -> Self {
        Self {
            connection,
            full,
            fact_budget,
            reuse: ReusePolicy::default(),
            files: HashMap::new(),
            writer: None,
            changed: false,
            resolution: Resolution::None,
            parsed: 0,
            reused: 0,
        }
    }
    pub fn unfinished(&self) -> bool {
        !self.files.is_empty()
    }
    pub fn accept(
        &mut self,
        event: Event,
        cancel: &Cancellation,
        progress: &mut dyn FnMut(&Progress) -> Result<()>,
    ) -> Result<()> {
        match event {
            Event::Progress(value) => progress(&value)?,
            Event::Failed {
                record,
                issue,
                message,
                native,
            } => {
                self.resolution.reset_all(self.connection)?;
                progress(&Progress::task(
                    "input_failed",
                    record.path.to_string_lossy().into_owned(),
                    Some(message.clone()),
                ))?;
                index::failures::replace(
                    self.connection,
                    record,
                    issue,
                    &message,
                    native.as_deref(),
                )?;
                self.changed = true;
                self.parsed += 1;
            }
            Event::Remove { path } => {
                self.connection.execute(
                    "INSERT OR IGNORE INTO deleted_files SELECT id FROM files WHERE path=?1",
                    [path_bytes(&path)],
                )?;
            }
            Event::Begin {
                record,
                mut change,
                partial,
            } => {
                if partial.is_some() || index::failures::was_partial(self.connection, &record.path)?
                {
                    self.resolution.reset_all(self.connection)?;
                    change = super::super::events::FileChange::Replace;
                }
                let context = super::start::file(
                    self.connection,
                    &record,
                    &change,
                    self.full,
                    &mut self.reuse,
                    &mut self.resolution,
                    self.fact_budget,
                )?;
                self.files.insert(record.path, context);
                self.changed = true;
            }
            Event::Facts { path, facts } => {
                let context = self
                    .files
                    .get_mut(path.as_ref())
                    .ok_or_else(|| Error::operation("fact arrived outside its file transaction"))?;
                let sink = match &mut self.writer {
                    Some(writer) => writer,
                    slot @ None => slot.insert(index::FactWriter::new(self.connection)?),
                };
                for (language, fact) in facts {
                    cancel.check()?;
                    sink.persist(context, language, fact)?;
                }
            }
            Event::End {
                path,
                fingerprints,
                native,
                fragments,
                partial,
            } => {
                let context = self.files.remove(path.as_ref()).ok_or_else(|| {
                    Error::operation("file completion has no matching extraction")
                })?;
                if context.call_reuse {
                    index::body_reuse::finish(self.connection, context.id)?;
                }
                index::native::replace(self.connection, context.id, native.as_deref())?;
                index::failures::fragments(self.connection, context.id, &fragments)?;
                self.partial(context.id, partial, &path, progress)?;
                super::super::progress::native(native.as_deref(), &path, progress)?;
                if self.resolution == Resolution::Selected
                    && !context.preserves_definitions()
                    && context.addition.is_none()
                    && !index::invalidation::safe_file(self.connection, context.id)?
                {
                    self.resolution.reset_all(self.connection)?;
                }
                self.connection.execute(
                    "UPDATE files SET facts_hash=?1,environment_hash=?2 WHERE id=?3",
                    params![fingerprints.all, fingerprints.environment, context.id],
                )?;
                self.parsed += 1;
                progress(&Progress {
                    completed_files: self.parsed + self.reused,
                    source_bytes: 0,

                    ..Progress::phase("parse")
                })?;
            }
            Event::UnchangedFacts {
                record,
                span,
                scope_ends,
                native,
                fragments,
                partial,
            } => {
                self.connection.execute(
                    "INSERT OR IGNORE INTO seen_files VALUES(?1)",
                    [path_bytes(&record.path)],
                )?;
                record.refresh_unchanged(self.connection, span, &scope_ends)?;
                let id: i64 = self.connection.query_row(
                    "SELECT id FROM files WHERE path=?1",
                    [path_bytes(&record.path)],
                    |r| r.get(0),
                )?;
                if index::failures::was_partial(self.connection, &record.path)? != partial.is_some()
                {
                    self.resolution.reset_all(self.connection)?;
                }
                index::native::replace(self.connection, id, native.as_deref())?;
                index::failures::fragments(self.connection, id, &fragments)?;
                self.partial(id, partial, &record.path, progress)?;
                super::super::progress::native(native.as_deref(), &record.path, progress)?;
                self.changed = true;
                self.parsed += 1;
                progress(&Progress {
                    completed_files: self.parsed + self.reused,
                    source_bytes: 0,

                    ..Progress::phase("parse")
                })?;
            }
            Event::Reuse { record, update } => {
                self.connection.execute(
                    "INSERT OR IGNORE INTO seen_files VALUES(?1)",
                    [path_bytes(&record.path)],
                )?;
                if update {
                    record.refresh_metadata(self.connection)?;
                }
                self.reused += 1;
            }
            Event::Skip { record, reason } => {
                self.connection.execute(
                    "INSERT OR IGNORE INTO seen_files VALUES(?1)",
                    [path_bytes(&record.path)],
                )?;
                if index::inert_inputs::unchanged_kind(self.connection, &record)? {
                    record.refresh_metadata(self.connection)?;
                    self.changed = true;
                    return Ok(());
                }
                self.resolution.reset_all(self.connection)?;
                let id = record.persist(self.connection)?;
                index::clear_file(self.connection, id)?;
                self.connection.execute(
                    "INSERT INTO diagnostics(file_id,code,message) VALUES(?1,?2,?3)",
                    params![id, reason, reason],
                )?;
                self.changed = true;
            }
        }
        Ok(())
    }
}

impl Writer<'_, '_> {
    fn partial(
        &self,
        file: i64,
        span: Option<crate::domain::source::SourceSpan>,
        path: &std::path::Path,
        progress: &mut dyn FnMut(&Progress) -> Result<()>,
    ) -> Result<()> {
        index::failures::partial(self.connection, file, span)?;
        if let Some(span) = span {
            progress(&Progress::task(
                "syntax_partial",
                path.to_string_lossy().into_owned(),
                Some(format!(
                    "recovered syntax at {}:{}; uncertain relationships remain candidates",
                    span.line, span.column
                )),
            ))?;
        }
        Ok(())
    }
}
