// Purpose: 부분 파일 반영 후 복구와 외부 편집 충돌 보호 검증.
// Pattern: Integration Test.
// Usage: engine 회귀 테스트가 중단 시점의 저장소 fixture를 재개.
// Related: crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;
use std::fs;

fn interrupted_restore(
    p: &Project,
) -> std::result::Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    p.write("main.rs", "fn old_main() {}")?;
    p.write("second.rs", "fn old_second() {}")?;
    p.build(false)?;
    p.write("main.rs", "fn new_main() {}")?;
    p.write("second.rs", "fn new_second() {}")?;
    p.build(false)?;
    let connection = rusqlite::Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    connection.execute_batch("BEGIN; INSERT INTO checkout VALUES(1,1,NULL,NULL); INSERT INTO checkout_files SELECT w.path,w.blob,w.mode,w.kind,e.blob,e.mode,e.kind,0 FROM working w JOIN entries e ON e.path=w.path AND e.checkpoint=1; COMMIT;")?;
    fs::write(p.root.join("main.rs"), "fn old_main() {}")?;
    connection.execute(
        "UPDATE checkout_files SET done=1 WHERE path=?1",
        [b"main.rs".as_slice()],
    )?;
    Ok(connection)
}

#[test]
fn interrupted_restore_resumes_and_publishes_exactly_one_checkpoint() -> TestResult {
    let p = Project::new()?;
    let connection = interrupted_restore(&p)?;
    let result = p.history(HistoryRequest::List {
        refresh: Refresh::None,
    })?;
    assert_eq!(result.rows.len(), 3);
    assert_eq!(
        fs::read_to_string(p.root.join("main.rs"))?,
        "fn old_main() {}"
    );
    assert_eq!(
        fs::read_to_string(p.root.join("second.rs"))?,
        "fn old_second() {}"
    );
    assert_eq!(
        connection.query_row("SELECT count(*) FROM checkout", [], |r| r.get::<_, u64>(0))?,
        0
    );
    assert_eq!(
        p.history(HistoryRequest::List {
            refresh: Refresh::None
        })?
        .rows
        .len(),
        3
    );
    Ok(())
}

#[test]
fn recovery_preserves_external_edits_and_stops_before_more_file_changes() -> TestResult {
    let p = Project::new()?;
    let connection = interrupted_restore(&p)?;
    fs::write(p.root.join("main.rs"), "fn external_edit() {}")?;
    assert!(
        p.history(HistoryRequest::List {
            refresh: Refresh::None
        })
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(p.root.join("main.rs"))?,
        "fn external_edit() {}"
    );
    assert_eq!(
        fs::read_to_string(p.root.join("second.rs"))?,
        "fn new_second() {}"
    );
    assert_eq!(
        connection.query_row("SELECT count(*) FROM checkpoints", [], |r| r
            .get::<_, u64>(0))?,
        2
    );
    assert_eq!(
        connection.query_row("SELECT count(*) FROM checkout", [], |r| r.get::<_, u64>(0))?,
        1
    );
    Ok(())
}

#[test]
fn doctor_detects_corrupted_source_even_when_analysis_is_cached() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn intact() {}")?;
    p.build(false)?;
    let connection = rusqlite::Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    connection.execute(
        "UPDATE chunk_data SET content=?1",
        [b"corrupted bytes".as_slice()],
    )?;
    let mut result = Collected::default();
    let error = p
        .engine
        .status(&p.options(), true, &Cancellation::default(), &mut result)
        .expect_err("content integrity must be checked");
    assert!(error.to_string().contains("checksum"));
    assert_eq!(
        fs::read_to_string(p.root.join("main.rs"))?,
        "fn intact() {}"
    );
    Ok(())
}

#[test]
fn cached_analysis_cannot_be_attributed_to_a_different_repository() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn source() {}")?;
    p.build(false)?;
    let connection = rusqlite::Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    connection.execute("UPDATE repository SET identity='replacement-store'", [])?;
    let error = p
        .query(Query::Symbols {
            file: "main.rs".into(),
        })
        .expect_err("cache belongs to an earlier repository");
    assert_eq!(error.kind, ErrorKind::NotFound);
    let mut sink = Collected::default();
    p.engine.query(
        &p.options(),
        &Query::Symbols {
            file: "main.rs".into(),
        },
        &QueryOptions::default(),
        &Cancellation::default(),
        &mut sink,
    )?;
    assert_eq!(
        sink.metadata
            .ok_or("metadata")?
            .history
            .ok_or("history")?
            .repository,
        "replacement-store"
    );
    Ok(())
}
