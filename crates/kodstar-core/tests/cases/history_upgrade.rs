// Purpose: 기존 이력을 자동 변경하지 않는 명시적 업그레이드 계약 검증.
// Pattern: Integration Test.
// Usage: engine 테스트가 실제 저장소를 형식 1 fixture로 전환 후 호출.
// Related: crates/kodstar-core/src/application/history/mod.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn current_format_repack_preserves_checkpoint_identity_and_actual_restore() -> TestResult {
    let p = Project::new()?;
    let initial = "fn original() {}\n".repeat(100);
    p.write("main.rs", &initial)?;
    p.build(false)?;
    p.write("main.rs", &initial.replace("original", "changed"))?;
    p.build(false)?;
    let db = rusqlite::Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    encoding::register(&db)?;
    let identities = |db: &rusqlite::Connection| -> rusqlite::Result<Vec<(i64, String, u64)>> {
        db.prepare("SELECT id,hash,size FROM blobs ORDER BY id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect()
    };
    let before = identities(&db)?;
    p.history(HistoryRequest::Upgrade)?;
    assert_eq!(before, identities(&db)?);
    p.history(HistoryRequest::Restore {
        revision: "c1".into(),
        path: None,
    })?;
    assert_eq!(std::fs::read_to_string(p.root.join("main.rs"))?, initial);
    db.execute_batch(
        "CREATE TABLE user_data(value TEXT); INSERT INTO user_data VALUES('preserved');",
    )?;
    assert!(p.history(HistoryRequest::Upgrade).is_err());
    assert_eq!(
        db.query_row("SELECT value FROM user_data", [], |r| r.get::<_, String>(0))?,
        "preserved"
    );
    Ok(())
}

#[test]
fn shared_v2_history_requires_explicit_upgrade_and_keeps_all_content() -> TestResult {
    let p = Project::new()?;
    let content = "// source\n".repeat(1000) + "fn saved() {}\n";
    p.write("main.rs", &content)?;
    p.build(false)?;
    let db = rusqlite::Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    encoding::register(&db)?;
    db.execute_batch("CREATE TABLE old_data(hash BLOB PRIMARY KEY,lines INTEGER,first_newline INTEGER,last_newline INTEGER,content BLOB) WITHOUT ROWID;
      INSERT INTO old_data SELECT d.hash,d.lines,d.first_newline,d.last_newline,kodstar_content(d.content,d.codec,d.size,b.content,b.codec,b.size) FROM chunk_data d LEFT JOIN chunk_data b ON b.id=d.base;
      CREATE TABLE old_refs(blob INTEGER,part INTEGER,hash BLOB,PRIMARY KEY(blob,part)) WITHOUT ROWID;
      INSERT INTO old_refs SELECT r.blob,r.part,d.hash FROM chunk_refs r JOIN chunk_data d ON d.id=r.chunk;
      DROP VIEW chunks; DROP TABLE chunk_refs; DROP TABLE chunk_data;
      ALTER TABLE old_data RENAME TO chunk_data;
      CREATE TABLE chunk_refs(blob INTEGER REFERENCES blobs(id),part INTEGER,hash BLOB REFERENCES chunk_data(hash),PRIMARY KEY(blob,part)) WITHOUT ROWID;
      INSERT INTO chunk_refs SELECT * FROM old_refs; DROP TABLE old_refs;
      CREATE VIEW chunks AS SELECT r.blob,r.part,d.content FROM chunk_refs r JOIN chunk_data d ON d.hash=r.hash;
      PRAGMA user_version=2;")?;
    drop(db);
    let error = p
        .build(false)
        .expect_err("old shared format changed without explicit upgrade");
    assert!(error.to_string().contains("history upgrade"));
    p.history(HistoryRequest::Upgrade)?;
    let db = rusqlite::Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    encoding::register(&db)?;
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
        4
    );
    let saved: Vec<u8> = db.query_row("SELECT content FROM chunks", [], |r| r.get(0))?;
    assert_eq!(saved, content.as_bytes());
    assert_eq!(
        db.query_row("SELECT count(*) FROM checkpoints", [], |r| r
            .get::<_, u64>(0))?,
        1
    );
    assert_eq!(
        db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))?,
        "ok"
    );
    assert_eq!(std::fs::read_to_string(p.root.join("main.rs"))?, content);
    Ok(())
}

#[test]
fn legacy_history_requires_explicit_upgrade_and_preserves_its_checkpoint() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn preserved() {}")?;
    let first = p.build(false)?;
    p.history(HistoryRequest::Save {
        name: "saved".into(),
    })?;
    let db = p.root.join(".kodstar/repository.sqlite");
    let connection = rusqlite::Connection::open(&db)?;
    encoding::register(&connection)?;
    let identity: String =
        connection.query_row("SELECT identity FROM repository", [], |r| r.get(0))?;
    connection.execute_batch("CREATE TABLE legacy_chunks(blob INTEGER,part INTEGER,content BLOB,PRIMARY KEY(blob,part)) WITHOUT ROWID;
        INSERT INTO legacy_chunks SELECT * FROM chunks; DROP VIEW chunks; DROP TABLE chunk_refs; DROP TABLE chunk_data;
        ALTER TABLE legacy_chunks RENAME TO chunks; PRAGMA user_version=1;")?;
    drop(connection);
    let error = p
        .engine
        .init(
            &p.options(),
            InitOptions::default(),
            &Cancellation::default(),
            &mut |_| Ok(()),
        )
        .expect_err("legacy history was automatically converted");
    assert!(error.to_string().contains("history upgrade"), "{error}");
    let result = p.history(HistoryRequest::Upgrade)?;
    assert!(result.rows.iter().any(|r|matches!(r,QueryRow::History(HistoryRow::Operation{operation,checkpoint,..}) if operation=="upgrade" && checkpoint=="c1")));
    let connection = rusqlite::Connection::open(&db)?;
    encoding::register(&connection)?;
    assert_eq!(
        connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
        4
    );
    assert_eq!(
        connection.query_row("SELECT identity FROM repository", [], |r| r
            .get::<_, String>(0))?,
        identity
    );
    assert_eq!(
        connection.query_row("SELECT count(*) FROM checkpoints", [], |r| r
            .get::<_, u64>(0))?,
        1
    );
    assert_eq!(
        std::fs::read_to_string(p.root.join("main.rs"))?,
        "fn preserved() {}"
    );
    let again = p.build(false)?;
    assert_eq!(again.history.as_ref().ok_or("history")?.checkpoint, "c1");
    assert_eq!(again.generation, first.generation);
    p.history(HistoryRequest::Upgrade)?;
    Ok(())
}
