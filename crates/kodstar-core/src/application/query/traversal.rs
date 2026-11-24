// Purpose: 관계 필터와 메모리·디스크 탐색의 순서·한도 조정.
// Pattern: Coordinator.
// Usage: 조회 조정자가 선택한 방향과 한도 전달.
// Related: crates/kodstar-core/src/application/query/mod.rs, crates/kodstar-core/src/application/query/traversal/memory.rs, crates/kodstar-core/src/application/query/traversal/disk.rs.

mod disk;
mod graph;
mod memory;
mod order;
mod path_neighbors;
#[cfg(test)]
mod tests;
mod undirected;
mod visited;

use crate::infrastructure::index::cached::Sql;
use rusqlite::{Connection, OptionalExtension, params};
use visited::Visited;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::query::QueryOptions;
use crate::domain::relations::Direction;
use crate::domain::results::QuerySummary;

pub(super) fn setup(connection: &Connection, options: &QueryOptions, explain: bool) -> Result<()> {
    crate::infrastructure::index::adjacency::prepare(connection)?;
    connection.execute_batch("CREATE TEMP TABLE q_seeds(id INTEGER PRIMARY KEY);CREATE TEMP TABLE q_edges(id INTEGER, direction TEXT, depth INTEGER, PRIMARY KEY(id,direction));CREATE TEMP TABLE q_frontier(id INTEGER,direction TEXT,depth INTEGER,parent INTEGER,via INTEGER,file_path BLOB,line INTEGER,column_byte INTEGER,name TEXT,PRIMARY KEY(id,direction));CREATE INDEX q_frontier_next ON q_frontier(depth,file_path,line,column_byte,name,direction);CREATE TEMP TABLE q_seen(id INTEGER,direction TEXT,depth INTEGER,parent INTEGER,via INTEGER,PRIMARY KEY(id,direction));CREATE TEMP TABLE q_nodes(id INTEGER PRIMARY KEY);CREATE TEMP TABLE q_externals(name TEXT PRIMARY KEY);CREATE TEMP TABLE q_relations(name TEXT PRIMARY KEY);")?;
    for relation in &options.relations {
        connection.execute(
            "INSERT OR IGNORE INTO q_relations VALUES(?1)",
            [relation.as_str()],
        )?;
    }
    let states = if explain {
        "1"
    } else if options.include_candidates {
        "r.resolution IN ('resolved','candidate','ambiguous')"
    } else {
        "r.resolution='resolved'"
    };
    let external = if options.include_external || explain {
        "1"
    } else {
        "r.target_id IS NOT NULL"
    };
    connection.execute_batch(&format!("CREATE TEMP VIEW q_allowed AS SELECT r.id,r.fact_id,r.source_id,r.target_id,r.external_key,f.relation,f.start_byte FROM relationships r JOIN facts f ON f.id=r.fact_id WHERE {states} AND {external} AND ((NOT EXISTS(SELECT 1 FROM q_relations) AND f.relation NOT IN ('defines','contains')) OR f.relation IN (SELECT name FROM q_relations));"))?;
    Ok(())
}

pub(super) fn members(connection: &Connection) -> Result<()> {
    connection.execute_batch("WITH RECURSIVE members(id) AS (
        SELECT g.canonical_id FROM q_seeds q JOIN symbol_identities g ON g.symbol_id=q.id
        UNION SELECT child.canonical_id FROM members m JOIN symbol_identities owner ON owner.canonical_id=m.id
          JOIN symbols s ON s.owner_id=owner.symbol_id JOIN symbol_identities child ON child.symbol_id=s.id
      ) INSERT OR IGNORE INTO q_seeds SELECT id FROM members;")?;
    canonical_seeds(connection)?;
    Ok(())
}

pub(super) struct Limits {
    pub maximum: Option<usize>,
    pub memory_bytes: usize,
}

pub(super) fn traverse(
    connection: &Connection,
    direction: Direction,
    depth: Option<usize>,
    limits: Limits,
    cancel: &Cancellation,
    summary: &mut QuerySummary,
    target: Option<i64>,
) -> Result<()> {
    let sql = Sql::new(connection)?;
    let connection = &sql;
    canonical_seeds(connection)?;
    if target.is_some() {
        path_neighbors::prepare(connection)?;
    }
    if direction == Direction::Undirected {
        let target =
            target.ok_or_else(|| Error::operation("undirected path requires a destination"))?;
        return undirected::run(connection, depth, &limits, cancel, summary, target);
    }
    let mut visited = Visited::new(connection, limits.memory_bytes / 3, cancel);
    let mut seeds=connection.cached_prepare("SELECT s.id FROM q_seeds q JOIN query_nodes s ON s.id=q.id ORDER BY s.path,s.line,s.column_byte,s.qualified_name")?;
    for seed in seeds.query_map([], |row| row.get::<_, i64>(0))? {
        let seed = seed?;
        if !visited.admit(Some(seed), None, limits.maximum, summary)? {
            break;
        }
        for current in ["out", "in"] {
            if direction == Direction::Out && current == "in"
                || direction == Direction::In && current == "out"
            {
                continue;
            }
            if visited.enqueue(seed, current)? {
                connection.cached_execute("INSERT OR IGNORE INTO q_frontier SELECT s.id,?1,0,NULL,NULL,s.path,s.line,s.column_byte,s.qualified_name FROM query_nodes s WHERE s.id=?2",params![current,seed])?;
            }
        }
    }
    if !memory::run(
        connection,
        depth,
        &limits,
        &mut visited,
        cancel,
        summary,
        target,
    )? {
        disk::run(
            connection,
            depth,
            &limits,
            &mut visited,
            cancel,
            summary,
            target,
        )?;
    }
    Ok(())
}

fn canonical_seeds(connection: &Connection) -> Result<()> {
    connection.execute_batch("INSERT OR IGNORE INTO q_seeds SELECT g.canonical_id FROM q_seeds q JOIN symbol_identities g ON g.symbol_id=q.id; DELETE FROM q_seeds WHERE id IN (SELECT symbol_id FROM symbol_identities WHERE symbol_id<>canonical_id);")?;
    Ok(())
}

pub(super) fn restrict_to_path(
    connection: &Connection,
    target: i64,
    mode: Direction,
) -> Result<()> {
    connection.execute_batch(
        "CREATE TEMP TABLE q_path(id INTEGER PRIMARY KEY,direction TEXT,depth INTEGER);",
    )?;
    let mut current = target;
    let mut direction: Option<String> = None;
    loop {
        let step:Option<(Option<i64>,Option<i64>,String,usize)>=connection.query_row("SELECT parent,via,direction,depth FROM q_seen WHERE id=?1 AND (?2 IS NULL OR direction=?2) ORDER BY depth,direction LIMIT 1",params![current,direction],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?;
        let Some((parent, edge, current_direction, depth)) = step else {
            connection.execute("DELETE FROM q_edges", [])?;
            return Ok(());
        };
        if let Some(edge) = edge {
            connection.execute(
                "INSERT OR IGNORE INTO q_path VALUES(?1,?2,?3)",
                params![edge, current_direction, depth],
            )?;
        }
        let Some(parent) = parent else { break };
        current = parent;
        if mode != Direction::Undirected {
            direction = Some(current_direction);
        }
    }
    connection.execute_batch(
        "DELETE FROM q_edges;INSERT INTO q_edges SELECT id,direction,depth FROM q_path;",
    )?;
    Ok(())
}
