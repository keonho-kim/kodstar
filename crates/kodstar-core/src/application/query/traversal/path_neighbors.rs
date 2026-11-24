// Purpose: 최단 경로의 이웃별 첫 근거를 선택하고 중복 사용 위치를 정렬 전에 축약.
// Pattern: Streaming Projection.
// Usage: 방향별 탐색이나 방향 전환 탐색이 현재 정점과 관계 방향을 전달.
// Related: crates/kodstar-core/src/application/query/traversal/disk.rs, crates/kodstar-core/src/application/query/traversal/undirected.rs.

use super::graph::Orientation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql;
use rusqlite::OptionalExtension;

pub(super) fn prepare(sql: &Sql<'_>) -> Result<()> {
    sql.execute_batch(
        "CREATE TEMP TABLE q_path_neighbors(node INTEGER, external TEXT, edge INTEGER,
             relation TEXT,start INTEGER,PRIMARY KEY(node,external)) WITHOUT ROWID;",
    )?;
    Ok(())
}

pub(super) const ORDERED: &str =
    "SELECT n.edge,NULLIF(n.node,0),CASE WHEN n.node=0 THEN n.external END
     FROM q_path_neighbors n LEFT JOIN query_nodes s ON s.id=n.node
     ORDER BY n.relation,s.path,s.line,s.column_byte,s.qualified_name,n.start,n.edge";

pub(super) fn collect(sql: &Sql<'_>, id: i64, direction: Orientation) -> Result<()> {
    let (from, to) = endpoints(direction);
    sql.cached_execute("DELETE FROM q_path_neighbors", [])?;
    sql.cached_execute(
        &format!(
            "INSERT INTO q_path_neighbors
             SELECT COALESCE(a.{to},0),COALESCE(a.external_key,''),a.id,a.relation,a.start_byte
             FROM q_allowed a WHERE a.{from}=?1 AND (a.{to} IS NOT NULL OR a.external_key IS NOT NULL)
             ON CONFLICT(node,external) DO UPDATE SET edge=excluded.edge,relation=excluded.relation,start=excluded.start
             WHERE (excluded.relation,excluded.start,excluded.edge)<(relation,start,edge)"
        ),
        [id],
    )?;
    Ok(())
}

pub(super) fn direct(
    sql: &Sql<'_>,
    current: i64,
    target: i64,
    direction: Orientation,
) -> Result<Option<i64>> {
    let (from, to) = endpoints(direction);
    Ok(sql.cached_row(
        &format!("SELECT id FROM q_allowed WHERE {from}=?1 AND {to}=?2 ORDER BY relation,start_byte,id LIMIT 1"),
        [current,target],
        |row| row.get(0),
    ).optional()?)
}

fn endpoints(direction: Orientation) -> (&'static str, &'static str) {
    match direction {
        Orientation::Out => ("source_id", "target_id"),
        Orientation::In => ("target_id", "source_id"),
    }
}
