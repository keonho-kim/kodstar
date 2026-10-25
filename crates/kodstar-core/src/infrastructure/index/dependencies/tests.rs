// Purpose: 압축 의존성의 한도·손상 거부와 부분 파일 공개의 보존 검증.
// Pattern: Unit Test.
// Usage: index dependency adapter의 의미·SQL 경계 회귀 검사.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/mod.rs.

use super::*;

#[test]
fn gaps_preserve_maximum_ids_and_found_bits() -> Result<()> {
    let mut bytes = Vec::new();
    codec::push(&mut bytes, 1, 0, false)?;
    codec::push(&mut bytes, 3, 1, true)?;
    codec::push(&mut bytes, i64::MAX, 3, false)?;
    assert_eq!(
        codec::json(&bytes, 3)?,
        format!("[[1,0],[3,1],[{},0]]", i64::MAX)
    );
    assert!(bytes.len() < 16);
    Ok(())
}

#[test]
fn malformed_or_unbounded_blocks_are_rejected() {
    for (bytes, count) in [
        (vec![], 0),
        (vec![], 1),
        (vec![0], 1),
        (vec![2, 2], 1),
        (vec![0x82, 0], 1),
        (vec![255; 11], 1),
        (vec![2; 129], 129),
    ] {
        assert!(codec::json(&bytes, count).is_err(), "{bytes:?}");
    }
}

#[test]
fn bounded_publication_preserves_both_lookup_outcomes_and_deletion() -> Result<()> {
    let connection = Connection::open_in_memory()?;
    register(&connection)?;
    super::super::packed::register(&connection)?;
    connection.execute_batch(super::super::dictionary::SCHEMA)?;
    connection.execute_batch("PRAGMA trusted_schema=OFF;
        CREATE TABLE files(id INTEGER PRIMARY KEY); CREATE TABLE facts(id INTEGER PRIMARY KEY,file_id INTEGER);
        CREATE TABLE lookup_rows(route_group INTEGER GENERATED ALWAYS AS ((key_id-1)/128) VIRTUAL,key_id INTEGER,file_id INTEGER,part INTEGER,count INTEGER,payload BLOB,PRIMARY KEY(key_id,file_id,part)) WITHOUT ROWID; CREATE VIEW lookup_blocks AS SELECT * FROM lookup_rows;
        CREATE TEMP TABLE resolution_dependencies(fact_id INTEGER,kind TEXT,lookup_key TEXT,found INTEGER);
        INSERT INTO files VALUES(1),(2);
        WITH RECURSIVE ids(id) AS (VALUES(1) UNION ALL SELECT id+1 FROM ids WHERE id<300)
        INSERT INTO facts SELECT id,CASE WHEN id<290 THEN 1 ELSE 2 END FROM ids;
        INSERT INTO resolution_dependencies SELECT id,'name','target',id%2 FROM facts;")?;
    publish(&connection)?;
    assert_eq!(
        connection.query_row("SELECT count(*) FROM lookup_blocks", [], |r| r
            .get::<_, i64>(0))?,
        4
    );
    let sql = "SELECT sum(json_array_length(kodstar_postings(payload,count))) FROM lookup_blocks";
    assert_eq!(connection.query_row(sql, [], |r| r.get::<_, i64>(0))?, 300);
    connection.execute("DELETE FROM lookup_rows WHERE file_id=1", [])?;
    assert_eq!(connection.query_row(sql, [], |r| r.get::<_, i64>(0))?, 11);
    Ok(())
}

#[test]
fn selected_fact_replacement_preserves_other_consumers_of_each_key() -> Result<()> {
    let c = Connection::open_in_memory()?;
    register(&c)?;
    super::super::packed::register(&c)?;
    c.execute_batch(super::super::dictionary::SCHEMA)?;
    c.execute_batch("CREATE TABLE files(id INTEGER PRIMARY KEY);
        CREATE TABLE facts(id INTEGER PRIMARY KEY,file_id INTEGER);
        CREATE TABLE lookup_rows(route_group INTEGER GENERATED ALWAYS AS ((key_id-1)/128) VIRTUAL,key_id INTEGER,file_id INTEGER,part INTEGER,count INTEGER,payload BLOB,PRIMARY KEY(key_id,file_id,part)) WITHOUT ROWID; CREATE VIEW lookup_blocks AS SELECT * FROM lookup_rows;
        CREATE TEMP TABLE resolution_dependencies(fact_id INTEGER,kind TEXT,lookup_key TEXT,found INTEGER);
        CREATE TEMP TABLE changed_files(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE dependent_files(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE affected_facts(id INTEGER PRIMARY KEY);
        CREATE VIEW fact_lookups AS SELECT json_extract(j.value,'$[0]') AS fact_id,b.key_id
            FROM lookup_blocks b,json_each(kodstar_postings(b.payload,b.count)) j;
        INSERT INTO files VALUES(1); INSERT INTO facts VALUES(1,1),(2,1),(3,1);
        INSERT INTO resolution_dependencies VALUES(1,'name','target',1),(2,'name','target',0),(1,'name','other',0),(3,'name','other',1);")?;
    publish(&c)?;
    c.execute_batch(
        "INSERT INTO dependent_files VALUES(1); INSERT INTO affected_facts VALUES(1);
        DELETE FROM resolution_dependencies;",
    )?;
    remove_selected(&c)?;
    assert_eq!(
        c.query_row("SELECT sum(count) FROM lookup_blocks", [], |r| r
            .get::<_, usize>(0))?,
        2
    );
    c.execute_batch(
        "INSERT INTO resolution_dependencies VALUES(1,'name','target',0),(1,'name','other',1);",
    )?;
    publish(&c)?;
    verify(&c)?;
    let rows = c.prepare("SELECT k.lookup_key,kodstar_postings(b.payload,b.count) FROM lookup_blocks b JOIN lookup_keys k ON k.id=b.key_id ORDER BY k.lookup_key")?
        .query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert_eq!(
        rows,
        vec![
            ("other".into(), "[[1,1],[3,1]]".into()),
            ("target".into(), "[[1,0],[2,0]]".into())
        ]
    );
    Ok(())
}
