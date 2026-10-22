// Purpose: 이력 결과에 마지막 정상 분석과 실패 상태 연결.
// Pattern: Query.
// Usage: history 명령이 분석 성공을 요구하지 않고 상태를 표시.
// Related: crates/kodstar-core/src/application/history/mod.rs.

use crate::domain::{cancellation::Cancellation, diagnostics::ErrorKind, history::HistoryState};
use crate::infrastructure::{cache::CacheStore, index};
use std::path::Path;

pub(super) fn read(
    store: &CacheStore,
    root: &Path,
    state: &mut HistoryState,
    cancel: &Cancellation,
) {
    let result = (|| {
        let cache = store.acquire(root, false, cancel)?;
        let connection = index::open(
            &cache.directory.join("index.sqlite"),
            root,
            false,
            2 * 1024 * 1024,
        )?;
        index::require_history(&connection, &state.repository, None)?;
        let (checkpoint, error): (Option<String>, Option<String>) =
            connection.query_row("SELECT checkpoint,last_error FROM metadata", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        state.analysis_checkpoint = checkpoint;
        state.analysis_error = error;
        Ok::<_, crate::domain::diagnostics::Error>(())
    })();
    if let Err(error) = result
        && error.kind != ErrorKind::NotFound
    {
        state.analysis_error = Some(error.to_string());
    }
}
