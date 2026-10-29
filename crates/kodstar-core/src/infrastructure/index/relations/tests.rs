// Purpose: 압축 관계의 복수 대상·nullable 원문·ID와 손상 경계 검증.
// Pattern: Storage Test.
// Usage: relation codec과 실제 SQLite adapter를 검증.
// Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs.

use super::*;

#[test]
fn multiple_use_sites_external_and_unresolved_are_lossless() -> Result<()> {
    let rows = vec![
        vec![
            1.into(),
            10.into(),
            2.into(),
            3.into(),
            Value::Null,
            "resolved".to_string().into(),
            4.into(),
            Value::Null,
        ],
        vec![
            2.into(),
            9.into(),
            2.into(),
            Value::Null,
            "外部".to_string().into(),
            "candidate".to_string().into(),
            4.into(),
            5.into(),
        ],
        vec![
            3.into(),
            11.into(),
            2.into(),
            Value::Null,
            Value::Null,
            "unresolved".to_string().into(),
            4.into(),
            5.into(),
        ],
    ];
    let rows = rows
        .into_iter()
        .map(Relation::from_values)
        .collect::<Result<Vec<_>>>()?;
    let bytes = codec::encode(&rows)?;
    assert_eq!(codec::decode(&bytes, 3, 2)?, rows);
    assert_eq!(
        codec::select(&bytes, 3, 2, &[(0, Value::Integer(2))])?,
        vec![Relation::from_values(vec![
            2.into(),
            9.into(),
            2.into(),
            Value::Null,
            "外部".to_string().into(),
            "candidate".to_string().into(),
            4.into(),
            5.into()
        ])?]
    );
    assert!(codec::select(&bytes[..bytes.len() - 1], 3, 2, &[(0, Value::Integer(1))]).is_err());
    assert!(codec::decode(&bytes[..bytes.len() - 1], 3, 2).is_err());
    assert!(codec::decode(&bytes, 129, 2).is_err());
    Ok(())
}

#[test]
fn indexed_reads_mutations_and_cascades_share_the_same_transaction() -> Result<()> {
    let c = Connection::open_in_memory()?;
    parents(&c)?;
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;
        INSERT INTO files_rows VALUES(1);INSERT INTO symbols_rows VALUES(1),(2),(3);INSERT INTO facts_rows VALUES(10,1),(11,1);")?;
    super::super::packed::register(&c)?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch(
        "INSERT INTO relation_texts VALUES(1,'fixture');
        INSERT INTO relationships VALUES(1,10,1,2,NULL,'resolved','fixture',NULL);
        INSERT INTO relationships VALUES(2,10,1,3,NULL,'ambiguous','fixture',NULL);
        INSERT INTO relationships VALUES(3,11,1,2,NULL,'resolved','fixture',NULL);",
    )?;
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM relationships WHERE target_id=2",
            [],
            |r| r.get::<_, usize>(0)
        )?,
        2
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM relationships WHERE source_id=1 AND fact_id=10",
            [],
            |r| r.get::<_, usize>(0)
        )?,
        2
    );
    c.execute_batch(
        "BEGIN; UPDATE relationship_rows SET id=id+100; DELETE FROM facts_rows WHERE id=11; ROLLBACK;",
    )?;
    assert_eq!(
        c.query_row("SELECT sum(id) FROM relationships", [], |r| r
            .get::<_, usize>(0))?,
        6
    );
    c.execute("DELETE FROM symbols_rows WHERE id=2", [])?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM relationships", [], |r| r
            .get::<_, usize>(0))?,
        1
    );
    let block: i64 = c.query_row("SELECT id FROM relation_blocks LIMIT 1", [], |r| r.get(0))?;
    let (file, source, rows) = super::load(&c, block)?;
    let writes = c.total_changes();
    super::store(&c, block, file, source, &rows)?;
    assert_eq!(
        c.total_changes(),
        writes,
        "identical blocks and routes were rewritten"
    );
    super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    Ok(())
}

#[test]
fn repeated_partial_publication_reuses_space_and_preserves_unrelated_ids() -> Result<()> {
    let c = Connection::open_in_memory()?;
    parents(&c)?;
    c.execute_batch("PRAGMA foreign_keys=ON;
        INSERT INTO files_rows VALUES(1); INSERT INTO symbols_rows VALUES(1),(2);
        WITH RECURSIVE ids(id) AS (VALUES(1) UNION ALL SELECT id+1 FROM ids WHERE id<100)
        INSERT INTO facts_rows SELECT id,1 FROM ids;
        CREATE TEMP TABLE resolution_relationships(id INTEGER PRIMARY KEY,fact_id INTEGER,source_id INTEGER,target_id INTEGER,external_key TEXT,resolution TEXT,rule TEXT,reason TEXT);")?;
    super::super::packed::register(&c)?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch("INSERT INTO resolution_relationships SELECT id,id,1,2,NULL,'resolved','fixture',NULL FROM facts;")?;
    publish(&c)?;
    let original: i64 = c.query_row("SELECT id FROM relationships WHERE fact_id=50", [], |r| {
        r.get(0)
    })?;
    for _ in 0..20 {
        c.execute_batch("INSERT INTO relation_prior SELECT * FROM relationship_rows WHERE fact_id IN (1,2);
            DELETE FROM relationship_rows WHERE fact_id IN (1,2);
            DELETE FROM resolution_relationships;
            INSERT INTO resolution_relationships SELECT id,id,1,2,NULL,'resolved','fixture',NULL FROM facts WHERE id IN (1,2);")?;
        publish(&c)?;
        assert_eq!(
            c.query_row("SELECT count(*) FROM relationships", [], |r| r
                .get::<_, usize>(0))?,
            100
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM relation_blocks", [], |r| r
                .get::<_, usize>(0))?,
            1
        );
        assert_eq!(
            c.query_row("SELECT max(id) FROM relationships", [], |r| r
                .get::<_, i64>(0))?,
            100
        );
        assert_eq!(
            c.query_row("SELECT id FROM relationships WHERE fact_id=50", [], |r| r
                .get::<_, i64>(
                0
            ))?,
            original
        );
    }
    super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    Ok(())
}

fn parents(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE files_rows(id INTEGER PRIMARY KEY);
        CREATE TABLE symbols_rows(id INTEGER PRIMARY KEY);
        CREATE TABLE facts_rows(id INTEGER PRIMARY KEY,file_id INTEGER);
        CREATE VIEW files AS SELECT * FROM files_rows;
        CREATE VIEW symbols AS SELECT * FROM symbols_rows;
        CREATE VIEW facts AS SELECT * FROM facts_rows;",
    )
}

#[test]
fn malformed_interval_ends_cannot_create_unbounded_logical_rows() -> Result<()> {
    let c = Connection::open_in_memory()?;
    parents(&c)?;
    c.execute_batch("INSERT INTO files_rows VALUES(1);INSERT INTO symbols_rows VALUES(1),(2);INSERT INTO facts_rows VALUES(1,1);")?;
    super::super::packed::register(&c)?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch(
        "INSERT INTO relationships VALUES(1,1,1,2,NULL,'resolved','fixture',NULL);
        UPDATE relation_id_ranges SET last='unbounded';",
    )?;
    let mut calls = 0;
    c.progress_handler(
        1000,
        Some(move || {
            calls += 1;
            calls > 20
        }),
    )?;
    let error = super::verify(&c).expect_err("malformed interval accepted");
    assert!(error.to_string().contains("routing interval"), "{error}");
    assert_eq!(
        c.query_row("SELECT count(*) FROM relation_ids", [], |r| r
            .get::<_, u64>(0))?,
        0
    );
    Ok(())
}
