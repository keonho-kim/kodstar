// Purpose: 분석 세대와 선택 소스 상태의 공통 metadata 구성.
// Pattern: Query.
// Usage: 일반 조회와 변경 영향 비교가 같은 출력 계약 사용.
// Related: crates/kodstar-core/src/application/query/mod.rs.

use crate::application::workspace::SelectedProject;
use crate::domain::{
    diagnostics::{Error, Result},
    query::Refresh,
    results::{MaintenanceReport, QueryMetadata},
};
use crate::infrastructure::{cache::paths::encoding, index};
use rusqlite::Connection;

pub(crate) fn read(
    connection: &Connection,
    selected: &SelectedProject,
    refresh: Refresh,
    maintenance: Option<MaintenanceReport>,
) -> Result<QueryMetadata> {
    let (generation, indexed_at, relation_registry_version): (i64, u64, u32) = connection
        .query_row(
            "SELECT generation,indexed_at,relation_registry_version FROM metadata",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
    if generation == 0 {
        return Err(Error::not_found(
            "no completed index found; run kodstar init first",
        ));
    }
    let coverage = index::coverage(connection)?;
    Ok(QueryMetadata {
        history: selected.history.clone(),
        maintenance,
        schema_version: 2,
        relation_registry_version,
        engine_version: crate::VERSION,
        root: Some(selected.root.to_string_lossy().into_owned()),
        root_encoding: encoding(&selected.root),
        generation: Some(generation),
        indexed_at: Some(indexed_at),
        freshness: if selected.historical {
            "checkpoint"
        } else {
            refresh.as_str()
        },
        unresolved_count: coverage.unresolved,
        coverage,
    })
}
