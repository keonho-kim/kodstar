// Purpose: 변경된 include·누락 입력의 소비 C/C++ 파일을 보존 delta에 추가.
// Pattern: Incremental Dependency Selection.
// Usage: 입력 선택이 고정 체크포인트 delta를 준비한 뒤 호출.
// Related: crates/kodstar-core/src/application/analysis/incremental/inputs.rs, crates/kodstar-core/src/infrastructure/clang/inputs.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    native::Observation,
};
use crate::infrastructure::cache::paths::{path_bytes, path_from_bytes};
use rusqlite::Connection;
use std::{path::Path, rc::Rc};

pub(super) fn prepare(
    snapshot: &Connection,
    previous: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    snapshot.execute_batch("CREATE TEMP TABLE IF NOT EXISTS analysis_extra_inputs(path BLOB PRIMARY KEY) WITHOUT ROWID;DELETE FROM analysis_extra_inputs;")?;
    let pending: bool =
        snapshot.query_row("SELECT EXISTS(SELECT 1 FROM snapshot_changes)", [], |r| {
            r.get(0)
        })?;
    if !pending {
        return Ok(());
    }
    let mut q=snapshot.prepare("SELECT path FROM snapshot_changes WHERE before_blob IS NOT after_blob OR before_attributes IS NOT after_attributes")?;
    let mut rows = q.query([])?;
    let mut consumers = previous.prepare(
        "SELECT f.path FROM native_inputs n JOIN files f ON f.id=n.file_id WHERE n.path=?1",
    )?;
    let mut insert = snapshot.prepare("INSERT OR IGNORE INTO analysis_extra_inputs VALUES(?1)")?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let path = path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        // A file addition/removal may change an observed ancestor directory's existence.
        for path in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
            let Some(path) = path.to_str() else { continue };
            let mut found = consumers.query([path])?;
            while let Some(row) = found.next()? {
                cancel.check()?;
                insert.execute([row.get::<_, Vec<u8>>(0)?])?;
            }
        }
    }
    Ok(())
}

pub(super) fn changed(
    previous: &Connection,
    snapshot: &Rc<Connection>,
    path: &Path,
    cancel: &Cancellation,
) -> Result<bool> {
    let mut q=previous.prepare("SELECT n.path,n.kind,n.hash FROM native_inputs n JOIN files f ON f.id=n.file_id WHERE f.path=?1")?;
    let mut rows = q.query([path_bytes(path)])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let path: String = row.get(0)?;
        let expected = match row.get::<_, i64>(1)? {
            0 => Observation::Missing,
            1 => Observation::Directory,
            2 => Observation::File(row.get(2)?),
            _ => return Err(Error::operation("invalid native input observation")),
        };
        let actual = crate::infrastructure::clang::observe(snapshot, Path::new(&path))?;
        if actual != expected {
            return Ok(true);
        }
    }
    Ok(false)
}
