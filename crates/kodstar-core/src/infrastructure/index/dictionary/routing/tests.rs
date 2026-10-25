// Purpose: route 경계·분리·회수 중 live lookup 보호와 rollback 검증.
// Pattern: Storage Regression Test.
// Usage: routing과 dictionary writer를 실제 압축 schema에서 함께 실행.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/routing/mod.rs, crates/kodstar-core/src/infrastructure/index/dictionary/mod.rs.

use super::*;

fn database() -> Result<Connection> {
    let c = Connection::open_in_memory()?;
    c.pragma_update(None, "foreign_keys", true)?;
    super::super::super::packed::register(&c)?;
    c.execute_batch(super::super::SCHEMA)?;
    c.execute_batch(
        "CREATE TABLE files_rows(id INTEGER PRIMARY KEY); INSERT INTO files_rows VALUES(1);",
    )?;
    c.execute_batch(include_str!("../../dependencies/schema.sql"))?;
    Ok(c)
}

#[test]
fn live_ids_survive_splits_and_lost_routes_force_rollback() -> Result<()> {
    let c = database()?;
    let block = super::super::store(
        &c,
        0,
        "module",
        &[(1, "a".into()), (128, "b".into()), (129, "c".into())],
    )?;
    c.execute(
        "INSERT INTO lookup_rows(key_id,file_id,part,count,payload) VALUES(128,1,0,1,X'02')",
        [],
    )?;
    c.execute_batch("BEGIN;")?;
    super::super::store(&c, block, "module", &[(1, "a".into())])?;
    assert!(collect(&c).is_err());
    c.execute_batch("ROLLBACK;")?;
    assert_eq!(find(&c, 128)?, Some(block));
    c.execute_batch("BEGIN;")?;
    super::super::store(&c, block, "module", &[(1, "a".into())])?;
    let moved = super::super::store(&c, 0, "module", &[(128, "b".into()), (129, "c".into())])?;
    collect(&c)?;
    c.execute_batch("COMMIT;")?;
    assert_eq!(find(&c, 1)?, Some(block));
    assert_eq!(find(&c, 128)?, Some(moved));
    assert_eq!(find(&c, 129)?, Some(moved));
    assert!(
        c.execute("DELETE FROM dictionary_routes WHERE id=0", [])
            .is_err()
    );
    super::super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    Ok(())
}

#[test]
fn empty_pages_are_reclaimed_and_damaged_pages_are_rejected() -> Result<()> {
    let c = database()?;
    let block = super::super::store(&c, 0, "module", &[(1, "a".into()), (129, "b".into())])?;
    super::super::store(&c, block, "module", &[(1, "a".into())])?;
    collect(&c)?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM dictionary_routes", [], |r| r
            .get::<_, usize>(0))?,
        1
    );
    assert_eq!(find(&c, 129)?, None);
    c.execute("UPDATE dictionary_routes SET payload=X'00' WHERE id=0", [])?;
    assert!(find(&c, 1).is_err());
    assert!(verify(&c).is_err());
    assert!(absolute(i64::MAX, 127).is_err());
    Ok(())
}

#[test]
fn maximum_key_keeps_indexed_page_deletion_and_missing_slot_guards() -> Result<()> {
    let c = database()?;
    let key = i64::MAX;
    let group = (key - 1) / 128;
    super::super::store(&c, 0, "name", &[(key, "last".into())])?;
    let sql = "INSERT INTO lookup_rows(key_id,file_id,part,count,payload) VALUES(?1,1,0,1,X'02')";
    assert!(c.execute(sql, [key - 1]).is_err());
    c.execute(sql, [key])?;
    assert!(
        c.execute("DELETE FROM dictionary_routes WHERE id=?1", [group])
            .is_err()
    );
    assert!(
        c.execute("UPDATE dictionary_routes SET id=0 WHERE id=?1", [group])
            .is_err()
    );
    c.execute("DELETE FROM lookup_rows WHERE key_id=?1", [key])?;
    c.execute("DELETE FROM dictionary_routes WHERE id=?1", [group])?;
    Ok(())
}
