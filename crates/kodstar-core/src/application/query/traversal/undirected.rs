// Purpose: 경로의 실제 edge 방향을 보존하는 방향 전환 최단 탐색.
// Pattern: Streaming Traversal.
// Usage: traversal 조정자가 path의 Undirected 모드에서 단일 시드로 호출.
// Related: crates/kodstar-core/src/application/query/traversal/disk.rs, crates/kodstar-core/src/application/query/traversal/visited.rs.

use rusqlite::OptionalExtension;

use crate::domain::{cancellation::Cancellation, diagnostics::Result, results::QuerySummary};
use crate::infrastructure::index::cached::Sql;

use super::{
    Limits, disk,
    graph::{Node, Orientation},
    path_neighbors,
    visited::Visited,
};

pub(super) fn run(
    sql: &Sql<'_>,
    depth: Option<usize>,
    limits: &Limits,
    cancel: &Cancellation,
    summary: &mut QuerySummary,
    target: i64,
) -> Result<()> {
    let mut visited = Visited::new(sql, limits.memory_bytes / 3, cancel);
    let seed = sql.cached_row("SELECT id FROM q_seeds", [], |r| r.get(0))?;
    if !visited.admit(Some(seed), None, limits.maximum, summary)? {
        return Ok(());
    }
    disk::enqueue(
        sql,
        Node {
            id: seed,
            direction: Orientation::Out,
            depth: 0,
            parent: None,
            via: None,
        },
    )?;
    let mut ordered = sql.cached_prepare(path_neighbors::ORDERED)?;
    loop {
        cancel.check()?;
        let current = sql
            .cached_row(&format!("{} LIMIT 1", disk::NEXT), [], disk::node)
            .optional()?;
        let Some(current) = current else {
            return Ok(());
        };
        sql.cached_execute("DELETE FROM q_frontier WHERE id=?1", [current.id])?;
        disk::seen(sql, current)?;
        if current.id == target {
            return Ok(());
        }
        if depth.is_some_and(|d| current.depth >= d) {
            continue;
        }
        // The frontier is ordered by depth and the established parent tie breakers.
        // A direct hop from this vertex completes a shortest path without expanding siblings.
        for direction in [Orientation::Out, Orientation::In] {
            if let Some(edge) = path_neighbors::direct(sql, current.id, target, direction)? {
                if visited.admit(Some(target), None, limits.maximum, summary)? {
                    disk::seen(
                        sql,
                        Node {
                            id: target,
                            direction,
                            depth: current.depth + 1,
                            parent: Some(current.id),
                            via: Some(edge),
                        },
                    )?;
                }
                return Ok(());
            }
        }
        for direction in [Orientation::Out, Orientation::In] {
            path_neighbors::collect(sql, current.id, direction)?;
            let mut rows = ordered.query([])?;
            while let Some(row) = rows.next()? {
                cancel.check()?;
                let Some(id) = row.get::<_, Option<i64>>(1)? else {
                    continue;
                };
                let known = sql.cached_row(
                    "SELECT EXISTS(SELECT 1 FROM q_seen WHERE id=?1)
                         OR EXISTS(SELECT 1 FROM q_frontier WHERE id=?1)",
                    [id],
                    |r| r.get::<_, bool>(0),
                )?;
                if known {
                    continue;
                }
                if !visited.admit(Some(id), None, limits.maximum, summary)? {
                    return Ok(());
                }
                let next = Node {
                    id,
                    direction,
                    depth: current.depth + 1,
                    parent: Some(current.id),
                    via: Some(row.get(0)?),
                };
                disk::enqueue(sql, next)?;
            }
        }
    }
}
