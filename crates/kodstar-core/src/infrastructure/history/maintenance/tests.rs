// Purpose: 명시적 형식 전환의 내용·참조 보존과 실패·중단 rollback 검증.
// Pattern: Unit Test.
// Usage: history upgrade가 소형 형식 1 fixture를 변환.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/mod.rs.

use super::*;
use rusqlite::{
    hooks::{AuthAction, AuthContext, Authorization},
    params,
};

pub(super) fn legacy() -> Result<Connection> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(include_str!("../schema.sql"))?;
    connection.execute_batch("CREATE TABLE chunks(blob INTEGER REFERENCES blobs(id),part INTEGER,content BLOB,PRIMARY KEY(blob,part)) WITHOUT ROWID; PRAGMA user_version=1; PRAGMA foreign_keys=ON;
        INSERT INTO checkpoints VALUES(1,NULL,123,1,1); UPDATE branches SET checkpoint=1;
        INSERT INTO names VALUES('saved',1); INSERT INTO checkout VALUES(1,1,NULL,NULL);
        INSERT INTO checkout_files VALUES(X'61',1,420,'file',2,420,'file',0);")?;
    let mut bytes = vec![b'a'; 65536 + 3];
    for id in [1, 2] {
        if id == 2 {
            bytes[65536] = b'b';
        }
        let hash = blake3::hash(&bytes).to_hex().to_string();
        connection.execute(
            "INSERT INTO blobs VALUES(?1,?2,?3)",
            params![id, hash, bytes.len()],
        )?;
        // Legacy short reads need normalization to fixed boundaries during conversion.
        for (part, chunk) in bytes.chunks(32760).enumerate() {
            connection.execute(
                "INSERT INTO chunks VALUES(?1,?2,?3)",
                params![id, part, chunk],
            )?;
        }
    }
    Ok(connection)
}

#[test]
fn upgrade_preserves_identifiers_labels_journal_and_all_bytes() -> Result<()> {
    let connection = legacy()?;
    let cancel = Cancellation::default();
    run(&connection, &cancel)?;
    assert_eq!(
        connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
        5
    );
    assert_eq!(
        connection.query_row("SELECT target FROM checkout", [], |r| r.get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        connection.query_row("SELECT after_blob FROM checkout_files", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    assert_eq!(
        connection.query_row("SELECT checkpoint FROM names WHERE name='saved'", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        connection.query_row("SELECT count(*) FROM chunk_data", [], |r| r
            .get::<_, i64>(0))?,
        3
    );
    for id in [1, 2] {
        let mut bytes = vec![b'a'; 65536 + 3];
        if id == 2 {
            bytes[65536] = b'b';
        }
        assert_eq!(
            super::super::content::blobs::read(&connection, id, bytes.len(), &cancel)?,
            bytes
        );
        let mut reader = super::super::content::reader::Reader::verified(&connection, id)?;
        let mut tail = [0; 5];
        assert_eq!(reader.read_at(65534, &mut tail, &cancel)?, 5);
        assert_eq!(&tail, &bytes[65534..]);
    }
    Ok(())
}

#[test]
fn invalid_content_and_cancellation_restore_the_original_schema() -> Result<()> {
    for corrupted in [true, false] {
        let connection = legacy()?;
        let cancel = Cancellation::default();
        if corrupted {
            connection.execute("UPDATE blobs SET hash='corrupt' WHERE id=2", [])?;
        } else {
            let token = cancel.clone();
            connection.authorizer(Some(move |context: AuthContext<'_>| {
                if matches!(
                    context.action,
                    AuthAction::CreateTable {
                        table_name: "chunk_refs"
                    }
                ) {
                    token.cancel();
                }
                Authorization::Allow
            }))?;
        }
        assert!(run(&connection, &cancel).is_err());
        connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)?;
        assert_eq!(
            connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
            1
        );
        assert_eq!(
            connection.query_row(
                "SELECT type FROM sqlite_schema WHERE name='chunks'",
                [],
                |r| r.get::<_, String>(0)
            )?,
            "table"
        );
        assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('chunk_data','legacy_chunks','chunk_refs')",[],|r|r.get::<_,u64>(0))?,0);
        assert_eq!(
            connection.query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, u64>(0))?,
            6
        );
    }
    Ok(())
}

#[test]
fn failed_compaction_preserves_the_completed_format_and_can_be_retried() -> Result<()> {
    let connection = legacy()?;
    let cancel = Cancellation::default();
    run(&connection, &cancel)?;
    connection.execute_batch("PRAGMA query_only=ON;")?;
    let error = compact(&connection, &cancel).expect_err("read-only compaction succeeded");
    assert!(error.to_string().contains("format is 5"), "{error}");
    connection.execute_batch("PRAGMA query_only=OFF;")?;
    compact(&connection, &cancel)?;
    assert_eq!(
        connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
        5
    );
    assert_eq!(
        connection.query_row("SELECT target FROM checkout", [], |r| r.get::<_, i64>(0))?,
        1
    );
    Ok(())
}

#[test]
fn unknown_tables_and_chunk_columns_are_preserved_without_conversion() -> Result<()> {
    for extra in [
        "CREATE TABLE private_notes(content TEXT); INSERT INTO private_notes VALUES('keep');",
        "ALTER TABLE chunks ADD COLUMN private_note TEXT DEFAULT 'keep';",
    ] {
        let connection = legacy()?;
        connection.execute_batch(extra)?;
        let error =
            run(&connection, &Cancellation::default()).expect_err("unknown data was converted");
        assert!(error.to_string().contains("preserved"), "{error}");
        assert_eq!(
            connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
            1
        );
        assert_eq!(
            connection.query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, u64>(0))?,
            6
        );
        let query = if extra.starts_with("CREATE") {
            "SELECT content FROM private_notes"
        } else {
            "SELECT private_note FROM chunks LIMIT 1"
        };
        assert_eq!(
            connection.query_row(query, [], |r| r.get::<_, String>(0))?,
            "keep"
        );
    }
    Ok(())
}

#[test]
fn database_full_preserves_source_history_and_the_original_failure() -> Result<()> {
    let connection = legacy()?;
    let pages: u64 = connection.pragma_query_value(None, "page_count", |r| r.get(0))?;
    connection.pragma_update(None, "max_page_count", pages)?;
    let error = run(&connection, &Cancellation::default())
        .expect_err("page limit did not prevent conversion");
    let source = std::error::Error::source(&error)
        .and_then(|e| e.downcast_ref::<rusqlite::Error>())
        .expect("SQLite failure");
    assert!(
        matches!(source,rusqlite::Error::SqliteFailure(code,_) if code.code==rusqlite::ErrorCode::DiskFull),
        "{source}"
    );
    assert_eq!(
        connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
        1
    );
    assert_eq!(
        connection.query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, u64>(0))?,
        6
    );
    Ok(())
}
