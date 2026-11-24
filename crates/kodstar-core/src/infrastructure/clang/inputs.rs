// Purpose: native VFS를 고정 체크포인트·입력 제외 정책에 제한하고 include 해시를 수집.
// Pattern: Input Adapter.
// Usage: native session이 실행 수명 동안 소유.
// Related: crates/kodstar-core/src/infrastructure/history/inputs/mod.rs, crates/kodstar-core/src/infrastructure/clang/mod.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, Result},
    native::Observation,
};
use crate::infrastructure::{
    cache::paths::path_bytes,
    history::{Snapshot, inputs},
};
use kodstar_clang::{InputError, Inputs, Source};
use rusqlite::OptionalExtension;
use std::{collections::BTreeMap, path::Path, rc::Rc};
mod catalog;

pub(super) struct ProjectInputs {
    pub connection: Rc<rusqlite::Connection>,
    pub dependencies: BTreeMap<String, Observation>,
    pub failure: Option<Error>,
    pub limit_issue: Option<kodstar_clang::Issue>,
    pub cancel: Cancellation,
    exclude: Vec<globset::GlobMatcher>,
    max_file: usize,
    bytes: usize,
    budget: usize,
    workspace: inputs::ReadWorkspace,
    catalog: catalog::Catalog,
}

impl ProjectInputs {
    pub fn new(
        snapshot: &Snapshot,
        max_file: usize,
        budget: usize,
        excludes: &[String],
        cancel: &Cancellation,
    ) -> Result<Self> {
        let exclude = excludes
            .iter()
            .map(|s| globset::Glob::new(s).map(|g| g.compile_matcher()))
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| Error::caused_by("compile native input exclusions", e))?;
        Ok(Self {
            connection: Rc::new(snapshot.open(cancel)?),
            dependencies: BTreeMap::new(),
            failure: None,
            limit_issue: None,
            cancel: cancel.clone(),
            exclude,
            max_file,
            bytes: 0,
            budget,
            workspace: inputs::ReadWorkspace::default(),
            catalog: catalog::Catalog::new(budget / 16),
        })
    }
    pub fn begin(&mut self, cancel: &Cancellation) {
        self.dependencies.clear();
        self.failure = None;
        self.limit_issue = None;
        self.bytes = 0;
        self.cancel = cancel.clone();
    }
    pub fn entry(&mut self, path: &str, content: bool) -> Result<Option<(usize, Option<Vec<u8>>)>> {
        if matches!(
            self.dependencies.get(path),
            Some(Observation::Missing | Observation::Directory)
        ) {
            return Ok(None);
        }
        let (entry, value) = self.catalog.get(&self.connection, path)?;
        let path = Path::new(path);
        let name = path
            .to_str()
            .ok_or_else(|| Error::operation("non-UTF-8 native include path"))?
            .to_owned();
        if !self.dependencies.contains_key(&name) {
            self.bytes = self.bytes.saturating_add(name.capacity() + 96);
            if self.bytes > self.budget {
                self.limit_issue = Some(kodstar_clang::Issue {
                    kind: kodstar_clang::IssueKind::Observations,
                    subject: String::new(),
                    used: self.bytes,
                    limit: self.budget,
                });
                return Err(Error::new(
                    ErrorKind::Limit,
                    "native include dependency budget exceeded",
                ));
            }
        }
        self.dependencies.insert(name, value);
        let Some(entry) = entry else { return Ok(None) };
        if entry.size() > self.max_file as u64 {
            self.limit_issue = Some(kodstar_clang::Issue {
                kind: kodstar_clang::IssueKind::InputFileBytes,
                subject: relative_name(path),
                used: usize::try_from(entry.size()).unwrap_or(usize::MAX),
                limit: self.max_file,
            });
            return Err(Error::new(
                ErrorKind::Limit,
                "native include exceeds max_file_bytes",
            ));
        }
        let bytes = if content {
            Some(entry.read_in(self.max_file, &self.cancel, &mut self.workspace)?)
        } else {
            None
        };
        Ok(Some((entry.size() as usize, bytes)))
    }
    fn lookup(&mut self, path: &str, content: bool) -> Result<Source> {
        self.cancel.check()?;
        if path == "/__kodstar_project" {
            return Ok(Source::Directory);
        }
        let Some(relative) = path.strip_prefix("/__kodstar_project/") else {
            return Err(Error::not_found("native path outside checkpoint"));
        };
        let mut normalized = std::path::PathBuf::new();
        for part in Path::new(relative).components() {
            match part {
                std::path::Component::Normal(name) => normalized.push(name),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir if normalized.pop() => {}
                _ => return Err(Error::not_found("native path escapes checkpoint")),
            }
        }
        let relative = normalized
            .to_str()
            .ok_or_else(|| Error::not_found("unsupported native path encoding"))?;
        let path = normalized.as_path();
        crate::infrastructure::history::paths::relative(path)?;
        if path
            .ancestors()
            .any(|p| self.exclude.iter().any(|g| g.is_match(p)))
        {
            return Err(Error::not_found("excluded native input"));
        }
        if let Some((size, bytes)) = self.entry(relative, content)? {
            return Ok(Source::File { size, bytes });
        }
        if self.dependencies.get(relative) == Some(&Observation::Directory) {
            Ok(Source::Directory)
        } else {
            Err(Error::not_found("native input is absent from checkpoint"))
        }
    }
}

fn relative_name(path: &Path) -> String {
    path.to_string_lossy().chars().take(512).collect()
}

pub(crate) fn observe(connection: &Rc<rusqlite::Connection>, path: &Path) -> Result<Observation> {
    let mut reader = inputs::Reader::new(connection)?;
    match reader.get(path) {
        Ok(entry) => Ok(Observation::File(entry.content_hash())),
        Err(e) if e.kind == ErrorKind::NotFound => missing_or_directory(connection, path),
        Err(e) => Err(e),
    }
}

fn missing_or_directory(connection: &rusqlite::Connection, path: &Path) -> Result<Observation> {
    let mut prefix = path_bytes(path);
    prefix.push(b'/');
    let mut end = prefix.clone();
    *end.last_mut()
        .ok_or_else(|| Error::operation("empty include prefix"))? += 1;
    let found: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM snapshot_files WHERE path>=?1 AND path<?2 LIMIT 1",
            rusqlite::params![prefix, end],
            |r| r.get(0),
        )
        .optional()?;
    Ok(if found.is_some() {
        Observation::Directory
    } else {
        Observation::Missing
    })
}
impl Inputs for ProjectInputs {
    fn read(&mut self, path: &str, content: bool) -> std::result::Result<Source, InputError> {
        match self.lookup(path, content) {
            Ok(value) => Ok(value),
            Err(e) if e.kind == ErrorKind::NotFound => Err(InputError::Missing),
            Err(e) => {
                let kind = match e.kind {
                    ErrorKind::Cancelled => InputError::Cancelled,
                    ErrorKind::Limit => InputError::Limit,
                    _ => InputError::Failure,
                };
                self.failure = Some(e);
                Err(kind)
            }
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}
