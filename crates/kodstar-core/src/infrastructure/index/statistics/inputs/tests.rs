// Purpose: 파일·고유 객체 차이 집계가 전체 조회 없이 동일한 수와 rollback을 보존하는지 검증.
// Pattern: Differential Storage Test.
// Usage: statistics input tracker의 실제 임시 trigger와 정체성 인덱스를 실행.
// Related: crates/kodstar-core/src/infrastructure/index/statistics/inputs.rs.

use super::*;
use rusqlite::functions::FunctionFlags;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn fixture() -> Result<(Connection, Arc<AtomicBool>)> {
    let c = Connection::open_in_memory()?;
    let deny = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&deny);
    c.create_scalar_function("observe_input", 1, FunctionFlags::SQLITE_UTF8, move |ctx| {
        if flag.load(Ordering::Relaxed) {
            return Err(rusqlite::Error::ModuleError(
                "unexpected full input scan".into(),
            ));
        }
        ctx.get::<String>(0)
    })?;
    c.execute_batch("CREATE TABLE metadata(singleton INTEGER PRIMARY KEY,generation INTEGER);
        INSERT INTO metadata VALUES(1,1);
        CREATE TABLE atoms(id INTEGER PRIMARY KEY,value TEXT);
        INSERT INTO atoms VALUES(1,'analyzed'),(2,'skipped'),(3,'unsupported'),(4,'auxiliary');
        CREATE TABLE files_rows(id INTEGER PRIMARY KEY,status INTEGER,size INTEGER);
        INSERT INTO files_rows VALUES(1,1,10),(2,3,20),(3,4,40);
        CREATE VIEW files AS SELECT f.id,observe_input(a.value) AS status,f.size FROM files_rows f JOIN atoms a ON a.id=f.status;
        CREATE TABLE symbols_rows(id INTEGER PRIMARY KEY,syntax_key TEXT,resolved_key TEXT,
            identity TEXT GENERATED ALWAYS AS(coalesce(resolved_key,syntax_key)) VIRTUAL);
        CREATE INDEX symbols_identity ON symbols_rows(identity);
        INSERT INTO symbols_rows(id,syntax_key) VALUES(1,'a'),(2,'a'),(3,'b');
        CREATE VIEW symbols AS SELECT observe_input(identity) AS key_blob FROM symbols_rows;
        CREATE TABLE coverage(singleton INTEGER PRIMARY KEY,generation INTEGER,scanned_files INTEGER,
            analyzed_files INTEGER,skipped_files INTEGER,unsupported_files INTEGER,source_bytes INTEGER,symbols INTEGER);
        INSERT INTO coverage VALUES(1,1,3,1,0,1,10,2);")?;
    Ok((c, deny))
}

#[test]
fn first_and_last_identity_changes_match_full_counts_without_scans() -> Result<()> {
    let (c, deny) = fixture()?;
    c.execute_batch("BEGIN;")?;
    begin(&c, true)?;
    c.execute_batch(
        "UPDATE files_rows SET size=15 WHERE id=1;
        UPDATE files_rows SET status=1 WHERE id=2;
        INSERT INTO files_rows VALUES(4,2,100);
        DELETE FROM files_rows WHERE id=3;
        DELETE FROM symbols_rows WHERE id=1;
        UPDATE symbols_rows SET resolved_key='a' WHERE id=3;
        INSERT INTO symbols_rows(id,syntax_key) VALUES(4,'c'),(5,'c');
        DELETE FROM symbols_rows WHERE id=4;",
    )?;
    deny.store(true, Ordering::Relaxed);
    let delta = read(&c)?;
    assert_eq!(
        delta,
        Counts {
            scanned: 3,
            analyzed: 2,
            skipped: 1,
            unsupported: 0,
            bytes: 35,
            symbols: 2
        }
    );
    finish(&c)?;
    deny.store(false, Ordering::Relaxed);
    assert_eq!(delta, read(&c)?);
    assert!(!c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_schema WHERE name LIKE 'statistics_%')",
        [],
        |r| r.get::<_, bool>(0)
    )?);
    c.execute_batch("COMMIT;")?;
    Ok(())
}

#[test]
fn rollback_removes_the_tracker_and_restores_input_coverage() -> Result<()> {
    let (c, _) = fixture()?;
    let before = read(&c)?;
    c.execute_batch("BEGIN;")?;
    begin(&c, true)?;
    c.execute_batch("DELETE FROM files_rows; DELETE FROM symbols_rows; ROLLBACK;")?;
    assert_eq!(before, read(&c)?);
    assert!(!c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_schema WHERE name LIKE 'statistics_%')",
        [],
        |r| r.get::<_, bool>(0)
    )?);
    Ok(())
}
