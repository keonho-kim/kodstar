// Purpose: 파일 이벤트를 경로 대기열 없이 변경·재스캔 신호로 합침.
// Pattern: Filesystem Adapter.
// Usage: application watch 실행이 수명 동안 adapter를 소유.
// Related: crates/kodstar-core/src/application/execution/watch.rs.

use crate::domain::diagnostics::{Error, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(crate) struct Changes {
    _watcher: Option<RecommendedWatcher>,
    dirty: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}

impl Changes {
    pub(crate) fn start(root: &Path, cache_home: &Path) -> Result<Self> {
        let dirty = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        // FSEvents requires UTF-8 paths. Metadata polling preserves non-UTF-8 root support.
        if cfg!(target_os = "macos") && root.to_str().is_none() {
            return Ok(Self {
                _watcher: None,
                dirty,
                error,
            });
        }
        let cache_home = cache_home
            .canonicalize()
            .map_err(|error| Error::caused_by("resolve watched cache exclusion", error))?;
        let history_home = root.join(".kodstar");
        let signal = Arc::clone(&dirty);
        let failure = Arc::clone(&error);
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            match result {
                Ok(event) => {
                    if !event.need_rescan()
                        && (matches!(event.kind, EventKind::Access(_))
                            || !event.paths.is_empty()
                                && event.paths.iter().all(|path| {
                                    path.starts_with(&cache_home) || path.starts_with(&history_home)
                                }))
                    {
                        return;
                    }
                    if event.need_rescan()
                        && let Ok(mut message) = failure.lock()
                    {
                        *message =
                            Some("filesystem event overflow requested a metadata rescan".into());
                    }
                }
                Err(error) => {
                    if let Ok(mut message) = failure.lock() {
                        let text = error.to_string();
                        let end = text
                            .char_indices()
                            .map(|(index, _)| index)
                            .take_while(|&index| index <= 4096)
                            .last()
                            .unwrap_or(0);
                        *message = Some(if text.len() <= 4096 {
                            text
                        } else {
                            text[..end].to_owned()
                        });
                    }
                }
            }
            signal.store(true, Ordering::Release);
        })
        .map_err(|error| Error::caused_by("initialize filesystem watcher", error))?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|error| Error::caused_by("watch project directory", error))?;
        Ok(Self {
            _watcher: Some(watcher),
            dirty,
            error,
        })
    }

    pub(crate) fn take_changes(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }
    pub(crate) fn polling_only(&self) -> bool {
        self._watcher.is_none()
    }
    pub(crate) fn take_error(&self) -> Result<Option<String>> {
        self.error
            .lock()
            .map(|mut error| error.take())
            .map_err(|_| Error::operation("watch error state lock poisoned"))
    }
}
