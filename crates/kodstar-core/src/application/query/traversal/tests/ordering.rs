// Purpose: 같은 길이의 경로를 발견 순서가 아닌 깊이별 정렬 순서로 선택하는 계약 검증.
// Pattern: Path Regression Test.
// Usage: traversal 테스트가 메모리와 디스크의 부모 경로를 비교.
// Related: crates/kodstar-core/src/application/query/traversal/order.rs, crates/kodstar-core/src/application/query/traversal/tests.rs.

use super::*;

#[test]
fn optimization_shared_paths_fit_a_bounded_frontier_sort() -> Result<()> {
    let c = fixture(32)?;
    c.execute(
        "UPDATE files_rows SET path=?1 WHERE id=1",
        [vec![b'a'; 2048]],
    )?;
    c.execute_batch("UPDATE symbols_rows SET file_id=1")?;
    let sql = crate::infrastructure::index::cached::Sql::new(&c)?;
    let mut queue = (1..=32)
        .rev()
        .map(|id| super::super::graph::Node {
            id,
            direction: super::super::graph::Orientation::Out,
            depth: 1,
            parent: Some(0),
            via: None,
        })
        .collect();
    assert!(super::super::order::sort(
        &sql,
        &mut queue,
        8192,
        &Cancellation::default()
    )?);
    assert_eq!(
        queue.iter().map(|node| node.id).collect::<Vec<_>>(),
        (1..=32).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn shortest_path_ties_follow_global_layer_order_instead_of_fifo_discovery() -> Result<()> {
    let execute = |bytes| -> Result<serde_json::Value> {
        let c = fixture(7)?;
        c.execute_batch(
            "DELETE FROM relationships;DELETE FROM facts;
            UPDATE symbols_rows SET file_id=1;UPDATE symbols_rows SET file_id=2 WHERE id=4;",
        )?;
        for (index, (source, target)) in [(1, 2), (1, 3), (2, 4), (3, 5), (4, 6), (5, 6)]
            .into_iter()
            .enumerate()
        {
            let id = index as i64 + 1;
            c.execute("INSERT INTO facts VALUES(?1,'calls',?1)", [id])?;
            c.execute(
                "INSERT INTO relationships VALUES(?1,?1,?2,?3,NULL,'resolved')",
                params![id, source, target],
            )?;
        }
        collect(&c, bytes, Direction::Out, Some(6), None, None)
    };
    let memory = execute(512 * 1024)?;
    assert_eq!(memory, execute(0)?);
    assert_eq!(
        memory["edges"],
        serde_json::json!([[2, "out", 1], [4, "out", 2], [6, "out", 3]])
    );
    Ok(())
}
