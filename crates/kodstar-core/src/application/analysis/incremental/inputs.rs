// Purpose: 정상 분석 시점의 순변화와 package 경계로 입력 탐색 범위를 선택.
// Pattern: Incremental Planning.
// Usage: discovery producer가 고정 이력 reader와 이전 분석 reader를 사용.
// Related: crates/kodstar-core/src/application/analysis/discovery.rs, crates/kodstar-core/src/infrastructure/history/changes/mod.rs.

use crate::application::workspace::SelectedProject;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::{
    cache::paths::path_from_bytes,
    history::{changes, checkpoints},
};
use rusqlite::Connection;
use std::rc::Rc;

pub(in crate::application::analysis) struct Selection {
    pub connection: Rc<Connection>,
    pub partial: bool,
    pub previous_files: u64,
}

pub(in crate::application::analysis) fn prepare(
    selected: &SelectedProject,
    previous: &Connection,
    full: bool,
    verify: bool,
    cancel: &Cancellation,
) -> Result<Selection> {
    let snapshot = selected
        .snapshot
        .as_ref()
        .ok_or_else(|| Error::operation("analysis requires a captured checkpoint"))?;
    let connection = Rc::new(snapshot.open(cancel)?);
    let mut selection = Selection {
        connection,
        partial: false,
        previous_files: 0,
    };
    if full || verify {
        return Ok(selection);
    }
    let prior: Option<String> =
        previous.query_row("SELECT checkpoint FROM metadata", [], |r| r.get(0))?;
    let Some(prior) = prior else {
        return Ok(selection);
    };
    let from = checkpoints::resolve(&selection.connection, &prior)?;
    let count = changes::prepare(&selection.connection, Some(from), snapshot.id, cancel)?;
    super::super::native_inputs::prepare(&selection.connection, previous, cancel)?;
    selection.previous_files = crate::infrastructure::index::coverage(previous)?.scanned_files;
    // A sequential inventory is cheaper when most existing paths changed.
    if count > selection.previous_files / 2 {
        return Ok(selection);
    }
    let mut q = selection.connection.prepare(
        "SELECT path FROM snapshot_changes WHERE
        (before_blob IS NOT NULL AND before_attributes&512=0) IS NOT
        (after_blob IS NOT NULL AND after_attributes&512=0)",
    )?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let path = path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        if super::super::module_identity::package_boundary(&path) {
            drop(rows);
            drop(q);
            return Ok(selection);
        }
    }
    drop(rows);
    drop(q);
    selection.partial = true;
    Ok(selection)
}
