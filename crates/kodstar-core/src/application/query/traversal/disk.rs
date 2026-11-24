// Purpose: 디스크 frontier와 두 탐색 경로의 동일한 이웃·한도·부모 기록.
// Pattern: Streaming Traversal.
// Usage: traversal 조정자와 memory 탐색이 같은 확장 계약을 사용.
// Related: crates/kodstar-core/src/application/query/traversal.rs, crates/kodstar-core/src/application/query/traversal/memory.rs.

use super::{
    Limits,
    graph::{Node, Orientation},
    path_neighbors,
    visited::Visited,
};
use crate::{
    domain::{cancellation::Cancellation, diagnostics::Result, results::QuerySummary},
    infrastructure::index::cached::Sql,
};
use rusqlite::{OptionalExtension, Row, params};

pub(super) fn node(row: &Row<'_>) -> rusqlite::Result<Node> {
    let direction: String = row.get(1)?;
    Ok(Node {
        id: row.get(0)?,
        direction: if direction == "in" {
            Orientation::In
        } else {
            Orientation::Out
        },
        depth: row.get(2)?,
        parent: row.get(3)?,
        via: row.get(4)?,
    })
}

pub(super) const NEXT: &str = "SELECT id,direction,depth,parent,via FROM q_frontier ORDER BY depth,file_path,line,column_byte,name,direction";

pub(super) fn enqueue(sql: &Sql<'_>, node: Node) -> Result<()> {
    sql.cached_execute("INSERT OR IGNORE INTO q_frontier SELECT s.id,?1,?2,?3,?4,s.path,s.line,s.column_byte,s.qualified_name FROM query_nodes s WHERE s.id=?5", params![node.direction.as_str(),node.depth,node.parent,node.via,node.id])?;
    Ok(())
}

pub(super) fn seen(sql: &Sql<'_>, node: Node) -> Result<()> {
    sql.cached_execute(
        "INSERT OR IGNORE INTO q_seen VALUES(?1,?2,?3,?4,?5)",
        params![
            node.id,
            node.direction.as_str(),
            node.depth,
            node.parent,
            node.via
        ],
    )?;
    Ok(())
}

#[derive(PartialEq)]
pub(super) enum Expansion {
    Complete,
    VisitLimit,
    Deferred,
}

pub(super) fn neighbors_query(direction: Orientation) -> String {
    let (source, neighbor) = match direction {
        Orientation::Out => ("source_id", "target_id"),
        Orientation::In => ("target_id", "source_id"),
    };
    format!(
        "SELECT a.id,a.{neighbor},a.external_key FROM q_allowed a LEFT JOIN query_nodes s ON s.id=a.{neighbor} WHERE a.{source}=?1 ORDER BY a.relation,s.path,s.line,s.column_byte,s.qualified_name,a.start_byte,a.id"
    )
}

pub(super) fn expand(
    sql: &Sql<'_>,
    current: Node,
    visited: &mut Visited<'_, '_>,
    (maximum, path): (Option<usize>, bool),
    cancel: &Cancellation,
    summary: &mut QuerySummary,
    mut consume: impl FnMut(Node, &mut Visited<'_, '_>) -> Result<bool>,
) -> Result<Expansion> {
    let mut statement = if path {
        path_neighbors::collect(sql, current.id, current.direction)?;
        sql.cached_prepare(path_neighbors::ORDERED)?
    } else {
        sql.cached_prepare(&neighbors_query(current.direction))?
    };
    let mut neighbors = if path {
        statement.query([])?
    } else {
        statement.query([current.id])?
    };
    while let Some(row) = neighbors.next()? {
        cancel.check()?;
        let edge: i64 = row.get(0)?;
        let next: Option<i64> = row.get(1)?;
        let external = row.get_ref(2)?;
        let external = external.as_str_or_null().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, external.data_type(), Box::new(e))
        })?;
        if !visited.admit(next, external, maximum, summary)? {
            return Ok(Expansion::VisitLimit);
        }
        if !path {
            sql.cached_execute(
                "INSERT OR IGNORE INTO q_edges VALUES(?1,?2,?3)",
                params![edge, current.direction.as_str(), current.depth + 1],
            )?;
        }
        if let Some(id) = next
            && !consume(
                Node {
                    id,
                    direction: current.direction,
                    depth: current.depth + 1,
                    parent: Some(current.id),
                    via: Some(edge),
                },
                visited,
            )?
        {
            return Ok(Expansion::Deferred);
        }
    }
    Ok(Expansion::Complete)
}

pub(super) fn run(
    sql: &Sql<'_>,
    depth: Option<usize>,
    limits: &Limits,
    visited: &mut Visited<'_, '_>,
    cancel: &Cancellation,
    summary: &mut QuerySummary,
    target: Option<i64>,
) -> Result<()> {
    loop {
        cancel.check()?;
        let next = sql
            .cached_row(&format!("{NEXT} LIMIT 1"), [], node)
            .optional()?;
        let Some(current) = next else {
            break;
        };
        sql.cached_execute(
            "DELETE FROM q_frontier WHERE id=?1 AND direction=?2",
            params![current.id, current.direction.as_str()],
        )?;
        seen(sql, current)?;
        if target == Some(current.id) {
            break;
        }
        if depth.is_some_and(|maximum| current.depth >= maximum) {
            continue;
        }
        let complete = expand(
            sql,
            current,
            visited,
            (limits.maximum, target.is_some()),
            cancel,
            summary,
            |next, visited| {
                if visited.enqueue(next.id, next.direction.as_str())? {
                    sql.cached_execute("INSERT OR IGNORE INTO q_frontier SELECT s.id,?1,?2,?3,?4,s.path,s.line,s.column_byte,s.qualified_name FROM query_nodes s WHERE s.id=?5 AND NOT EXISTS(SELECT 1 FROM q_seen v WHERE v.id=?5 AND v.direction=?1)",params![next.direction.as_str(),next.depth,next.parent,next.via,next.id])?;
                }
                Ok(true)
            },
        )?;
        if complete != Expansion::Complete {
            break;
        }
    }
    Ok(())
}
