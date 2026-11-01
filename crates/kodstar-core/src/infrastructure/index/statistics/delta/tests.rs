// Purpose: 중복 후보·삭제·새 fact의 통계 차이와 journal 한도 fallback 검증.
// Pattern: Storage Regression Test.
// Usage: bounded 통계를 전체 집계와 대조하며 무관한 관계 재조회를 거절.
// Related: crates/kodstar-core/src/infrastructure/index/statistics/delta.rs, crates/kodstar-core/src/infrastructure/index/statistics.rs.

use super::super::{Update, refresh};
use super::*;
use rusqlite::functions::FunctionFlags;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[test]
fn small_changes_match_full_counts_without_reading_unrelated_facts() -> Result<()> {
    let c = Connection::open_in_memory()?;
    let forbid = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&forbid);
    c.create_scalar_function(
        "checked_state",
        2,
        FunctionFlags::SQLITE_UTF8,
        move |context| {
            if flag.load(Ordering::Relaxed) && context.get::<i64>(0)? >= 10 {
                return Err(rusqlite::Error::ModuleError("unrelated graph scan".into()));
            }
            context.get::<String>(1)
        },
    )?;
    c.execute_batch("CREATE TABLE metadata(singleton INTEGER,generation INTEGER); INSERT INTO metadata VALUES(1,1);
        CREATE TABLE files(id INTEGER PRIMARY KEY,status TEXT,size INTEGER); INSERT INTO files VALUES(1,'analyzed',10),(2,'analyzed',100);
        CREATE TABLE facts(id INTEGER PRIMARY KEY,file_id INTEGER,source_id INTEGER,scope_id INTEGER,start_byte INTEGER,end_byte INTEGER,expression TEXT,relation TEXT);
        INSERT INTO facts VALUES(1,1,1,0,0,1,'a','calls'),(2,2,2,0,0,1,'b','calls'),(3,1,1,0,2,3,'c','calls');
        WITH RECURSIVE ids(id) AS (VALUES(10) UNION ALL SELECT id+1 FROM ids WHERE id<609)
        INSERT INTO facts SELECT id,2,2,0,id,id+1,'stable','calls' FROM ids;
        CREATE TABLE symbols(key_blob TEXT); INSERT INTO symbols VALUES('a'),('b');
        CREATE TABLE symbol_aliases(symbol_id INTEGER PRIMARY KEY,canonical_id INTEGER);
        CREATE TABLE raw_relations(fact_id INTEGER,resolution TEXT); CREATE INDEX relation_fact ON raw_relations(fact_id);
        INSERT INTO raw_relations VALUES(1,'ambiguous'),(1,'ambiguous'),(2,'resolved'),(3,'unresolved');
        INSERT INTO raw_relations SELECT id,'resolved' FROM facts WHERE id>=10;
        CREATE VIEW relationships AS SELECT fact_id,checked_state(fact_id,resolution) AS resolution FROM raw_relations;
        CREATE TABLE coverage(singleton INTEGER PRIMARY KEY,generation INTEGER,scanned_files INTEGER,analyzed_files INTEGER,skipped_files INTEGER,unsupported_files INTEGER,source_bytes INTEGER,symbols INTEGER,relationships INTEGER,unresolved INTEGER);
        CREATE TEMP TABLE changed_files(id INTEGER PRIMARY KEY); INSERT INTO changed_files VALUES(1);
        CREATE TEMP TABLE affected_facts(id INTEGER PRIMARY KEY); INSERT INTO affected_facts VALUES(2);")?;
    refresh(&c, 1, Update::All)?;
    reset(&c, true, 128)?;
    remember_file(&c, 1)?;
    c.execute_batch("DELETE FROM raw_relations WHERE fact_id IN (1,2,3);
        INSERT INTO raw_relations VALUES(1,'ambiguous'),(2,'ambiguous'),(2,'ambiguous'),(4,'resolved'),(5,'resolved');
        DELETE FROM facts WHERE id=3;
        INSERT INTO facts VALUES(4,1,1,0,2,3,'new','calls'),(5,1,1,0,4,5,'another','calls');")?;
    forbid.store(true, Ordering::Relaxed);
    refresh(&c, 2, Update::Selected)?;
    c.execute("UPDATE metadata SET generation=2", [])?;
    let partial = super::super::read(&c)?;
    assert_eq!((partial.relationships, partial.unresolved), (602, 2));
    forbid.store(false, Ordering::Relaxed);
    refresh(&c, 2, Update::All)?;
    let full = super::super::read(&c)?;
    assert_eq!(
        (partial.symbols, partial.relationships, partial.unresolved),
        (full.symbols, full.relationships, full.unresolved)
    );
    reset(&c, true, 128)?;
    remember_file(&c, 2)?;
    assert!(!enabled(&c)?);
    assert!(difference(&c)?.is_none());
    Ok(())
}
