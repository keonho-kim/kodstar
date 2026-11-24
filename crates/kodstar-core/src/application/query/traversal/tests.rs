// Purpose: 메모리·디스크 탐색의 순서·부모·한도·방향·오류 일치 검증.
// Pattern: Contract Test.
// Usage: core 단위 테스트가 실제 SQLite 작업 집합으로 실행.
// Related: crates/kodstar-core/src/application/query/traversal.rs.

use super::*;
mod ordering;
mod plans;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn fixture(wide: usize) -> Result<Connection> {
    let c = Connection::open_in_memory()?;
    c.execute_batch("CREATE TABLE files_rows(id INTEGER PRIMARY KEY,path BLOB);
        CREATE VIEW files AS SELECT * FROM files_rows;
        CREATE TABLE atoms(id INTEGER PRIMARY KEY,value TEXT);
        CREATE TABLE symbols_rows(id INTEGER PRIMARY KEY,file_id INTEGER,line INTEGER,column_byte INTEGER,qualified_id INTEGER);
        CREATE VIEW symbols AS SELECT s.id,s.file_id,s.line,s.column_byte,a.value qualified_name FROM symbols_rows s LEFT JOIN atoms a ON a.id=s.qualified_id;
        CREATE VIEW symbol_identities AS SELECT id symbol_id,id canonical_id FROM symbols;
        CREATE TABLE facts(id INTEGER PRIMARY KEY,relation TEXT,start_byte INTEGER);
        CREATE TABLE relationships(id INTEGER PRIMARY KEY,fact_id INTEGER,source_id INTEGER,target_id INTEGER,external_key TEXT,resolution TEXT);
        CREATE INDEX fixture_source ON relationships(source_id);
        CREATE INDEX fixture_target ON relationships(target_id);
        INSERT INTO files_rows VALUES(1,X'61'),(2,X'7A');")?;
    for id in 1..=wide.max(7) as i64 {
        c.execute(
            "INSERT INTO atoms VALUES(?1,?2)",
            params![id, format!("n{id}")],
        )?;
        c.execute(
            "INSERT INTO symbols_rows VALUES(?1,?2,?1,1,?1)",
            params![id, if id == 2 { 2 } else { 1 }],
        )?;
    }
    let edges = [
        (1, 2),
        (1, 3),
        (2, 4),
        (3, 4),
        (5, 1),
        (6, 2),
        (5, 7),
        (4, 1),
    ];
    for (index, (from, to)) in edges.into_iter().enumerate() {
        let id = index as i64 + 1;
        c.execute("INSERT INTO facts VALUES(?1,'calls',?1)", [id])?;
        c.execute(
            "INSERT INTO relationships VALUES(?1,?1,?2,?3,NULL,'resolved')",
            params![id, from, to],
        )?;
    }
    c.execute_batch("INSERT INTO facts VALUES(9,'calls',9);INSERT INTO relationships VALUES(9,9,1,NULL,'外部','resolved');")?;
    for id in 8..=wide as i64 {
        c.execute("INSERT INTO facts VALUES(?1,'calls',?1)", [id + 10])?;
        c.execute(
            "INSERT INTO relationships VALUES(?1,?1,1,?2,NULL,'resolved')",
            params![id + 10, id],
        )?;
    }
    setup(
        &c,
        &QueryOptions {
            include_external: true,
            ..QueryOptions::default()
        },
        false,
    )?;
    c.execute_batch("INSERT INTO q_seeds VALUES(1)")?;
    Ok(c)
}

fn run(
    bytes: usize,
    direction: Direction,
    target: Option<i64>,
    depth: Option<usize>,
    maximum: Option<usize>,
    wide: usize,
) -> Result<serde_json::Value> {
    let c = fixture(wide)?;
    collect(&c, bytes, direction, target, depth, maximum)
}

fn collect(
    c: &Connection,
    bytes: usize,
    direction: Direction,
    target: Option<i64>,
    depth: Option<usize>,
    maximum: Option<usize>,
) -> Result<serde_json::Value> {
    let mut summary = QuerySummary {
        complete: true,
        ..QuerySummary::default()
    };
    traverse(
        c,
        direction,
        depth,
        Limits {
            maximum,
            memory_bytes: bytes,
        },
        &Cancellation::default(),
        &mut summary,
        target,
    )?;
    if let Some(target) = target {
        restrict_to_path(c, target, direction)?;
    }
    let edges = c
        .prepare("SELECT id,direction,depth FROM q_edges ORDER BY id,direction")?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, usize>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let seen = c
        .prepare("SELECT id,direction,depth,parent,via FROM q_seen ORDER BY id,direction")?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, usize>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(serde_json::json!({"edges":edges,"seen":seen,"summary":summary}))
}

#[test]
fn bfs_and_disk_keep_order_parents_cycles_external_limits_and_directions() -> Result<()> {
    for direction in [Direction::Out, Direction::In, Direction::Both] {
        for (target, depth, maximum) in [
            (None, None, None),
            (Some(4), None, None),
            (None, Some(1), None),
            (None, None, Some(3)),
            (None, None, Some(1)),
        ] {
            let disk = run(0, direction, target, depth, maximum, 7)?;
            for bytes in [96 * 1024, 512 * 1024, 1024 * 1024] {
                assert_eq!(
                    run(bytes, direction, target, depth, maximum, 7)?,
                    disk,
                    "budget {bytes}"
                );
            }
        }
    }
    let path = run(512 * 1024, Direction::Out, Some(4), None, None, 7)?;
    assert_eq!(
        path["edges"],
        serde_json::json!([[2, "out", 1], [4, "out", 2]])
    );
    let both = run(512 * 1024, Direction::Both, None, Some(1), None, 7)?;
    assert!(
        !both["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge[0] == 6)
    );
    Ok(())
}

#[test]
fn a_wide_frontier_continues_on_disk_without_losing_the_current_expansion() -> Result<()> {
    assert_eq!(
        run(512 * 1024, Direction::Out, None, None, None, 1600)?,
        run(0, Direction::Out, None, None, None, 1600)?
    );
    assert_eq!(
        run(512 * 1024, Direction::Out, Some(4), None, Some(800), 1600)?,
        run(0, Direction::Out, Some(4), None, Some(800), 1600)?
    );
    Ok(())
}

#[test]
fn memory_bfs_avoids_per_node_frontier_writes_and_propagates_cancellation() -> Result<()> {
    for budget in [0, 512 * 1024] {
        let c = fixture(7)?;
        let deletes = Arc::new(AtomicUsize::new(0));
        let count = deletes.clone();
        c.update_hook(Some(
            move |action: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
                if table == "q_frontier" && action == rusqlite::hooks::Action::SQLITE_DELETE {
                    count.fetch_add(1, Ordering::Relaxed);
                }
            },
        ))?;
        let limits = Limits {
            maximum: None,
            memory_bytes: budget,
        };
        traverse(
            &c,
            Direction::Out,
            None,
            limits,
            &Cancellation::default(),
            &mut QuerySummary::default(),
            None,
        )?;
        assert_eq!(
            deletes.load(Ordering::Relaxed),
            if budget == 0 { 4 } else { 0 }
        );
    }
    let c = fixture(7)?;
    let cancel = Cancellation::default();
    let token = cancel.clone();
    c.update_hook(Some(
        move |_: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
            if table == "q_seen" {
                token.cancel();
            }
        },
    ))?;
    let error = traverse(
        &c,
        Direction::Out,
        None,
        Limits {
            maximum: None,
            memory_bytes: 512 * 1024,
        },
        &cancel,
        &mut QuerySummary::default(),
        None,
    )
    .unwrap_err();
    assert_eq!(error.kind, crate::domain::diagnostics::ErrorKind::Cancelled);
    Ok(())
}

#[test]
fn sqlite_read_failures_are_returned_before_a_graph_step() -> Result<()> {
    let c = fixture(7)?;
    c.execute_batch("DROP TABLE facts")?;
    assert!(
        traverse(
            &c,
            Direction::Out,
            None,
            Limits {
                maximum: None,
                memory_bytes: 512 * 1024
            },
            &Cancellation::default(),
            &mut QuerySummary::default(),
            None
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn sparse_ids_and_long_order_keys_continue_with_identical_disk_results() -> Result<()> {
    for sparse in [true, false] {
        let execute = |bytes| -> Result<serde_json::Value> {
            let c = fixture(40)?;
            if sparse {
                c.execute_batch("UPDATE symbols_rows SET id=id*65536;UPDATE relationships SET source_id=source_id*65536,target_id=target_id*65536;UPDATE q_seeds SET id=id*65536;")?;
            } else {
                c.execute(
                    "UPDATE atoms SET value=?1 WHERE id=2",
                    ["🚀".repeat(100000)],
                )?;
            }
            collect(&c, bytes, Direction::Out, None, None, None)
        };
        assert_eq!(execute(512 * 1024)?, execute(0)?);
    }
    Ok(())
}
