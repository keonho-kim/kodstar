// Purpose: 제한된 petgraph BFS를 실행하고 남은 작업을 디스크 frontier로 전달.
// Pattern: Bounded Traversal.
// Usage: traversal이 디스크 탐색 전에 시도하며 false이면 같은 상태에서 이어감.
// Related: crates/kodstar-core/src/application/query/traversal/graph.rs, crates/kodstar-core/src/application/query/traversal/disk.rs.

use super::{
    Limits, disk,
    graph::{Discovered, Layer, Node},
    order,
    visited::Visited,
};
use crate::{
    domain::{cancellation::Cancellation, diagnostics::Result, results::QuerySummary},
    infrastructure::index::cached::Sql,
};
use petgraph::visit::{Bfs, VisitMap};
use std::collections::VecDeque;

pub(super) fn run(
    sql: &Sql<'_>,
    depth: Option<usize>,
    limits: &Limits,
    visited: &mut Visited<'_, '_>,
    cancel: &Cancellation,
    summary: &mut QuerySummary,
    target: Option<i64>,
) -> Result<bool> {
    let bytes = limits.memory_bytes / 3;
    let capacity = (bytes / (6 * std::mem::size_of::<Node>())).min(4096);
    if capacity == 0 {
        return Ok(false);
    }
    let mut bfs = Bfs {
        stack: VecDeque::new(),
        discovered: Discovered::new(bytes),
    };
    let mut neighbors = Vec::new();
    if bfs.stack.try_reserve_exact(capacity).is_err()
        || neighbors.try_reserve_exact(capacity).is_err()
    {
        return Ok(false);
    }
    let buffers = (bfs.stack.capacity() + neighbors.capacity()) * std::mem::size_of::<Node>();
    let Some(order_bytes) = bytes.checked_sub(buffers) else {
        return Ok(false);
    };
    let mut read = sql.cached_prepare(disk::NEXT)?;
    let mut seeds = read.query([])?;
    while let Some(row) = seeds.next()? {
        cancel.check()?;
        let node = disk::node(row)?;
        if bfs.stack.len() == capacity {
            return Ok(false);
        }
        if !bfs.discovered.visit(node) {
            return Ok(false);
        }
        bfs.stack.push_back(node);
    }
    drop(seeds);
    drop(read);
    sql.execute_batch("DELETE FROM q_frontier")?;
    let mut remaining = bfs.stack.len();
    while let Some(&current) = bfs.stack.front() {
        cancel.check()?;
        disk::seen(sql, current)?;
        if target == Some(current.id) {
            return Ok(true);
        }
        neighbors.clear();
        if !depth.is_some_and(|maximum| current.depth >= maximum) {
            let complete = disk::expand(
                sql,
                current,
                visited,
                (limits.maximum, target.is_some()),
                cancel,
                summary,
                |next, _| {
                    if bfs.stack.len() - 1 + neighbors.len() >= capacity {
                        return Ok(false);
                    }
                    neighbors.push(next);
                    Ok(true)
                },
            )?;
            if complete == disk::Expansion::VisitLimit {
                return Ok(true);
            }
            if complete == disk::Expansion::Deferred {
                spill(sql, &bfs.stack, visited, cancel)?;
                return Ok(false);
            }
        }
        bfs.next(&Layer(&neighbors));
        if bfs.discovered.overflow {
            // Re-expanding this one node restores neighbors rejected by the bounded visit map.
            // Admission counts and q_edges are idempotent; earlier parents stay unchanged.
            disk::enqueue(sql, current)?;
            spill(sql, &bfs.stack, visited, cancel)?;
            return Ok(false);
        }
        remaining -= 1;
        if remaining == 0 {
            if !order::sort(sql, &mut bfs.stack, order_bytes, cancel)? {
                spill(sql, &bfs.stack, visited, cancel)?;
                return Ok(false);
            }
            remaining = bfs.stack.len();
        }
    }
    Ok(true)
}

fn spill(
    sql: &Sql<'_>,
    queue: &VecDeque<Node>,
    visited: &mut Visited<'_, '_>,
    cancel: &Cancellation,
) -> Result<()> {
    for &node in queue {
        cancel.check()?;
        visited.enqueue(node.id, node.direction.as_str())?;
        disk::enqueue(sql, node)?;
    }
    Ok(())
}
