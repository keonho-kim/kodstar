// Purpose: 의미가 같은 세대의 통계 갱신은 관계를 다시 읽지 않고 파일 상태만 반영.
// Pattern: Storage Regression Test.
// Usage: statistics의 부분 갱신과 정상 전체 집계를 대조.
// Related: crates/kodstar-core/src/infrastructure/index/statistics.rs.

use super::*;
use rusqlite::functions::FunctionFlags;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[test]
fn file_only_generations_preserve_graph_counts_without_reading_relationships() -> Result<()> {
    let c = Connection::open_in_memory()?;
    let forbid = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&forbid);
    c.create_scalar_function(
        "checked_state",
        1,
        FunctionFlags::SQLITE_UTF8,
        move |context| {
            if flag.load(Ordering::Relaxed) {
                return Err(rusqlite::Error::ModuleError("unexpected graph scan".into()));
            }
            context.get::<String>(0)
        },
    )?;
    c.execute_batch("CREATE TABLE metadata(singleton INTEGER,generation INTEGER); INSERT INTO metadata VALUES(1,1);
        CREATE TABLE files(status TEXT,size INTEGER); INSERT INTO files VALUES('analyzed',10),('unsupported',4);
        CREATE TABLE symbols(key_blob TEXT); INSERT INTO symbols VALUES('a'),('a'),('b');
        CREATE TABLE raw_relations(fact_id INTEGER,resolution TEXT);
        INSERT INTO raw_relations VALUES(1,'resolved'),(2,'ambiguous'),(2,'ambiguous'),(3,'unresolved');
        CREATE VIEW relationships AS SELECT fact_id,checked_state(resolution) AS resolution FROM raw_relations;
        CREATE TABLE coverage(singleton INTEGER PRIMARY KEY,generation INTEGER,scanned_files INTEGER,analyzed_files INTEGER,skipped_files INTEGER,unsupported_files INTEGER,source_bytes INTEGER,symbols INTEGER,relationships INTEGER,unresolved INTEGER);")?;
    refresh(&c, 1, Update::All)?;
    forbid.store(true, Ordering::Relaxed);
    c.execute_batch(
        "UPDATE files SET size=20 WHERE status='analyzed'; INSERT INTO files VALUES('skipped',9);",
    )?;
    refresh(&c, 2, Update::Files)?;
    c.execute("UPDATE metadata SET generation=2", [])?;
    let value = read(&c)?;
    assert_eq!(
        (
            value.scanned_files,
            value.analyzed_files,
            value.skipped_files,
            value.unsupported_files,
            value.source_bytes
        ),
        (3, 1, 1, 1, 20)
    );
    assert_eq!(
        (value.symbols, value.relationships, value.unresolved),
        (2, 1, 2)
    );
    forbid.store(false, Ordering::Relaxed);
    refresh(&c, 2, Update::All)?;
    let full = read(&c)?;
    assert_eq!(
        (full.symbols, full.relationships, full.unresolved),
        (value.symbols, value.relationships, value.unresolved)
    );
    Ok(())
}
