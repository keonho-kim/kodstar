// Purpose: 루트와 등록 프로젝트 선택.
// Pattern: Simple Module.
// Usage: Engine이 작업 시작 전에 호출.
// Related: crates/kodstar-core/src/application/engine.rs, crates/kodstar-core/src/application/workspace/settings.rs.

//! Project selection, configuration precedence, ignore policy, and module boundaries.

mod settings;

pub use settings::{Settings, SettingsOverrides};

use std::path::{Path, PathBuf};

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::infrastructure::cache::CacheStore;
use crate::infrastructure::cache::paths::canonical_directory;

#[derive(Clone, Debug)]
pub struct ProjectOptions {
    pub cwd: PathBuf,
    pub root: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub overrides: SettingsOverrides,
}

impl ProjectOptions {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            root: None,
            config: None,
            overrides: SettingsOverrides::default(),
        }
    }
}

pub(crate) struct SelectedProject {
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub settings: Settings,
}

pub(crate) enum Selection<'a> {
    Build(Option<&'a Path>),
    Existing,
}

pub(crate) fn select(
    store: &CacheStore,
    options: &ProjectOptions,
    selection: Selection<'_>,
    cancel: &Cancellation,
) -> Result<SelectedProject> {
    cancel.check()?;
    let cwd = canonical_directory(&options.cwd)?;
    let explicit = options
        .root
        .as_ref()
        .map(|root| canonical_directory(&cwd.join(root)))
        .transpose()?;
    let build_path = match selection {
        Selection::Build(Some(path)) => Some(canonical_directory(&cwd.join(path))?),
        _ => None,
    };
    if let (Some(root), Some(path)) = (&explicit, &build_path)
        && root != path
    {
        return Err(Error::argument(
            "build path and --root refer to different projects",
        ));
    }
    let root = if let Some(root) = explicit.or(build_path) {
        root
    } else if matches!(selection, Selection::Build(_)) {
        cwd.clone()
    } else {
        let mut selected = None;
        for ancestor in cwd.ancestors() {
            cancel.check()?;
            if store.registered(ancestor, cancel)? {
                selected = Some(ancestor.to_path_buf());
                break;
            }
        }
        selected.ok_or_else(|| {
            Error::not_found("no registered project found; run kodstar build first")
        })?
    };
    let config = options.config.as_ref().map(|path| cwd.join(path));
    let settings = Settings::load(&store.home, config.as_deref(), &options.overrides)?;
    Ok(SelectedProject {
        root,
        cwd,
        settings,
    })
}
