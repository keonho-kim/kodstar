// Purpose: 초기화 대상 확인과 로컬 데이터 제거의 공개 경계.
// Pattern: Use Case.
// Usage: CLI가 터미널 확인 후 Engine의 설치 수명 API 호출.
// Related: crates/kodstar-core/src/application/engine.rs, crates/kodstar-core/src/infrastructure/installation/removal.rs.

use super::{
    analysis::InitOptions,
    engine::Engine,
    workspace::{self, ProjectOptions, Selection},
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::{cache::locks::FileLock, history::Repository, installation};
use std::{fs, path::PathBuf};

/// Canonical initialization target; inspecting it does not create application data.
pub struct InitializationTarget {
    pub root: PathBuf,
    pub initialized: bool,
}

/// Exclusive removal ownership. Keep alive until installation files have been removed.
#[must_use = "keep the removal lease alive and call finish after installation cleanup"]
pub struct LocalDataRemoval {
    home: PathBuf,
    _lease: Option<FileLock>,
}

/// Keeps an editor transport alive without allowing concurrent uninstall.
#[must_use = "keep this lease alive for the editor session lifetime"]
pub struct LocalDataLease {
    _lease: Option<FileLock>,
}

impl Engine {
    pub fn hold_local_data(&self, cancel: &Cancellation) -> Result<LocalDataLease> {
        Ok(LocalDataLease {
            _lease: installation::lease(&self.cache, true, cancel)?,
        })
    }
    pub fn initialization_target(
        &self,
        project: &ProjectOptions,
        options: &InitOptions,
    ) -> Result<InitializationTarget> {
        let selected = workspace::select(
            &self.cache,
            project,
            Selection::Init(options.path.as_deref()),
            &Cancellation::default(),
        )?;
        Ok(InitializationTarget {
            initialized: Repository::exists(&selected.root),
            root: selected.root,
        })
    }

    /// Validate every owned data location before removing any source history.
    /// Caller must explicitly authorize removal of all registered checkpoint histories.
    pub fn remove_local_data(&self, cancel: &Cancellation) -> Result<LocalDataRemoval> {
        if !self
            .cache
            .home
            .try_exists()
            .map_err(|e| Error::io("inspect application home", &self.cache.home, e))?
        {
            return Ok(LocalDataRemoval {
                home: self.cache.home.clone(),
                _lease: None,
            });
        }
        self.cache.ensure_home()?;
        let lease = FileLock::try_acquire(&self.cache.home.join("installation.lock"), false)?
            .ok_or_else(|| {
                Error::operation(
                    "KodStar is in use; stop watch and editor sessions before uninstalling",
                )
            })?;
        installation::removal::run(&self.cache, cancel)?;
        Ok(LocalDataRemoval {
            home: self.cache.home.clone(),
            _lease: Some(lease),
        })
    }
}

impl LocalDataRemoval {
    /// Finish after the CLI has removed its receipt and shell blocks.
    pub fn finish(self) -> Result<()> {
        if self._lease.is_none() {
            return Ok(());
        }
        let path = self.home.join("installation.lock");
        if path.exists() {
            fs::remove_file(&path).map_err(|e| Error::io("remove installation lock", &path, e))?;
        }
        if self.home.exists() {
            fs::remove_dir(&self.home).map_err(|e| {
                Error::io(
                    "finish uninstall; remaining data was preserved",
                    &self.home,
                    e,
                )
            })?;
        }
        Ok(())
    }
}
