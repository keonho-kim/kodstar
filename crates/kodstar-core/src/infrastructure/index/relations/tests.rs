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
    let bytes = codec::encode(&rows)?;
    assert_eq!(codec::decode(&bytes, 3, 2)?, rows);
    assert!(codec::decode(&bytes[..bytes.len() - 1], 3, 2).is_err());
    assert!(codec::decode(&bytes, 129, 2).is_err());
    Ok(())
}

#[test]
fn indexed_reads_mutations_and_cascades_share_the_same_transaction() -> Result<()> {
    let c = Connection::open_in_memory()?;
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;
        CREATE TABLE files(id INTEGER PRIMARY KEY);CREATE TABLE symbols(id INTEGER PRIMARY KEY);
        CREATE TABLE facts(id INTEGER PRIMARY KEY,file_id INTEGER);
        INSERT INTO files VALUES(1);INSERT INTO symbols VALUES(1),(2),(3);INSERT INTO facts VALUES(10,1),(11,1);")?;
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
        "BEGIN; UPDATE relationship_rows SET id=id+100; DELETE FROM facts WHERE id=11; ROLLBACK;",
    )?;
    assert_eq!(
        c.query_row("SELECT sum(id) FROM relationships", [], |r| r
            .get::<_, usize>(0))?,
        6
    );
    c.execute("DELETE FROM symbols WHERE id=2", [])?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM relationships", [], |r| r
            .get::<_, usize>(0))?,
        1
    );
    super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    Ok(())
}
