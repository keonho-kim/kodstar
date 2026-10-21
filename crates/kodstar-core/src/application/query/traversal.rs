// Purpose: 관계 필터와 디스크 frontier·visited·경로 탐색.
// Pattern: Simple Module.
// Usage: 조회 조정자가 선택한 방향과 한도 전달.
// Related: crates/kodstar-core/src/application/query/mod.rs.

use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::query::QueryOptions;
use crate::domain::relations::Direction;
use crate::domain::results::QuerySummary;

pub(super) fn setup(connection: &Connection, options: &QueryOptions, explain: bool) -> Result<()> {
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
    connection.execute_batch(&format!("CREATE TEMP VIEW q_allowed AS SELECT r.id,r.source_id,r.target_id FROM relationships r JOIN facts f ON f.id=r.fact_id WHERE {states} AND {external} AND ((NOT EXISTS(SELECT 1 FROM q_relations) AND f.relation NOT IN ('defines','contains')) OR f.relation IN (SELECT name FROM q_relations));"))?;
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

struct FrontierEntry {
    id: i64,
    direction: String,
    depth: usize,
    parent: Option<i64>,
    via: Option<i64>,
}

pub(super) fn traverse(
    connection: &Connection,
    direction: Direction,
    depth: Option<usize>,
    max_nodes: Option<usize>,
    cancel: &Cancellation,
    summary: &mut QuerySummary,
    target: Option<i64>,
) -> Result<()> {
    canonical_seeds(connection)?;
    let mut seeds=connection.prepare("SELECT s.id FROM q_seeds q JOIN symbols s ON s.id=q.id JOIN files f ON f.id=s.file_id ORDER BY f.path,s.line,s.column_byte,s.qualified_name")?;
    for seed in seeds.query_map([], |row| row.get::<_, i64>(0))? {
        let seed = seed?;
        if !admit(connection, Some(seed), None, max_nodes, summary)? {
            break;
        }
        for current in ["out", "in"] {
            if direction == Direction::Out && current == "in"
                || direction == Direction::In && current == "out"
            {
                continue;
            }
            connection.execute("INSERT OR IGNORE INTO q_frontier SELECT s.id,?1,0,NULL,NULL,f.path,s.line,s.column_byte,s.qualified_name FROM symbols s JOIN files f ON f.id=s.file_id WHERE s.id=?2",params![current,seed])?;
        }
    }
    'search: loop {
        cancel.check()?;
        let next = connection.query_row(
            "SELECT id,direction,depth,parent,via FROM q_frontier ORDER BY depth,file_path,line,column_byte,name,direction LIMIT 1",
            [],
            |row| Ok(FrontierEntry {
                id: row.get(0)?, direction: row.get(1)?, depth: row.get(2)?,
                parent: row.get(3)?, via: row.get(4)?,
            }),
        ).optional()?;
        let Some(FrontierEntry {
            id,
            direction: current,
            depth: distance,
            parent,
            via,
        }) = next
        else {
            break;
        };
        connection.execute(
            "DELETE FROM q_frontier WHERE id=?1 AND direction=?2",
            params![id, current],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO q_seen VALUES(?1,?2,?3,?4,?5)",
            params![id, current, distance, parent, via],
        )?;
        if target == Some(id) {
            break;
        }
        if depth.is_some_and(|maximum| distance >= maximum) {
            continue;
        }
        let (source, neighbor) = if current == "out" {
            ("source_id", "target_id")
        } else {
            ("target_id", "source_id")
        };
        let mut statement=connection.prepare(&format!("SELECT a.id,a.{neighbor},r.external_key FROM q_allowed a JOIN relationships r ON r.id=a.id JOIN facts fact ON fact.id=r.fact_id LEFT JOIN symbols s ON s.id=a.{neighbor} LEFT JOIN files f ON f.id=s.file_id WHERE a.{source}=?1 ORDER BY fact.relation,f.path,s.line,s.column_byte,s.qualified_name,fact.start_byte,a.id"))?;
        let mut neighbors = statement.query([id])?;
        while let Some(row) = neighbors.next()? {
            cancel.check()?;
            let edge: i64 = row.get(0)?;
            let next: Option<i64> = row.get(1)?;
            let external: Option<String> = row.get(2)?;
            if !admit(connection, next, external.as_deref(), max_nodes, summary)? {
                break 'search;
            }
            connection.execute(
                "INSERT OR IGNORE INTO q_edges VALUES(?1,?2,?3)",
                params![edge, current, distance + 1],
            )?;
            if let Some(next) = next {
                connection.execute("INSERT OR IGNORE INTO q_frontier SELECT s.id,?1,?2,?3,?4,f.path,s.line,s.column_byte,s.qualified_name FROM symbols s JOIN files f ON f.id=s.file_id WHERE s.id=?5 AND NOT EXISTS(SELECT 1 FROM q_seen v WHERE v.id=?5 AND v.direction=?1)",params![current,distance+1,id,edge,next])?;
            }
        }
    }
    Ok(())
}

fn canonical_seeds(connection: &Connection) -> Result<()> {
    connection.execute_batch("INSERT OR IGNORE INTO q_seeds SELECT g.canonical_id FROM q_seeds q JOIN symbol_identities g ON g.symbol_id=q.id; DELETE FROM q_seeds WHERE id IN (SELECT symbol_id FROM symbol_identities WHERE symbol_id<>canonical_id);")?;
    Ok(())
}

fn admit(
    connection: &Connection,
    id: Option<i64>,
    external: Option<&str>,
    maximum: Option<usize>,
    summary: &mut QuerySummary,
) -> Result<bool> {
    let exists = if let Some(id) = id {
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM q_nodes WHERE id=?1)",
            [id],
            |row| row.get::<_, bool>(0),
        )?
    } else if let Some(name) = external {
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM q_externals WHERE name=?1)",
            [name],
            |row| row.get::<_, bool>(0),
        )?
    } else {
        return Ok(true);
    };
    if exists {
        return Ok(true);
    }
    if maximum.is_some_and(|limit| summary.visited_count >= limit as u64) {
        summary.complete = false;
        summary.truncated = true;
        if !summary.reasons.contains(&"visit_limit") {
            summary.reasons.push("visit_limit");
        }
        return Ok(false);
    }
    if let Some(id) = id {
        connection.execute("INSERT INTO q_nodes VALUES(?1)", [id])?;
    } else if let Some(name) = external {
        connection.execute("INSERT INTO q_externals VALUES(?1)", [name])?;
    }
    summary.visited_count += 1;
    Ok(true)
}

pub(super) fn restrict_to_path(connection: &Connection, target: i64) -> Result<()> {
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
        direction = Some(current_direction);
    }
    connection.execute_batch(
        "DELETE FROM q_edges;INSERT INTO q_edges SELECT id,direction,depth FROM q_path;",
    )?;
    Ok(())
}
