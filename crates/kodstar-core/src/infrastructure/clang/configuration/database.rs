// Purpose: 고정 snapshot compile database를 한 번 파싱하고 파일별 후보·중복을 보존.
// Pattern: Immutable Index.
// Usage: native session의 한 worker 수명에서 compile argument 선택이 사용.
// Related: crates/kodstar-core/src/infrastructure/clang/configuration.rs, crates/kodstar-core/src/infrastructure/clang/mod.rs.

use super::{Command, ProjectInputs, relative};
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(in crate::infrastructure::clang) struct Database {
    entries: BTreeMap<PathBuf, Option<Command>>,
}
impl Database {
    pub fn load(input: &mut ProjectInputs, root: &Path) -> Result<Self> {
        let (_, Some(bytes)) = input
            .entry("compile_commands.json", true)?
            .ok_or_else(|| Error::operation("compile database disappeared from immutable input"))?
        else {
            return Err(Error::operation("compile database content was not read"));
        };
        let commands: Vec<Command> = serde_json::from_slice(&bytes).map_err(|e| {
            Error::private_input(ErrorKind::Argument, "invalid compile_commands.json", e)
        })?;
        let mut entries = BTreeMap::new();
        for command in commands {
            let Some(file) = relative(root, &command.directory, &command.file) else {
                continue;
            };
            entries
                .entry(file)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(command));
        }
        Ok(Self { entries })
    }
    pub(super) fn select(&self, file: &Path) -> Option<Option<&Command>> {
        self.entries.get(file).map(Option::as_ref)
    }

    pub(super) fn owner(&self, file: &Path) -> Option<Option<PathBuf>> {
        let stem = file.file_stem()?;
        let mut candidates = self.entries.iter().filter(|(path, _)| {
            path.file_stem() == Some(stem)
                && path
                    .extension()
                    .is_some_and(|e| matches!(e.to_str(), Some("c" | "cc" | "cpp" | "cxx")))
        });
        let (owner, command) = candidates.next()?;
        Some((command.is_some() && candidates.next().is_none()).then(|| owner.clone()))
    }
}
