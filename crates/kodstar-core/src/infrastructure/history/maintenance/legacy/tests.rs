// Purpose: 공유 형식 전환의 원문 손상·취소·알 수 없는 데이터 보존 검증.
// Pattern: Unit Test.
// Usage: 형식 2·3 fixture의 명시 전환과 실패 rollback 확인.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/legacy.rs, crates/kodstar-core/src/infrastructure/history/maintenance/tests.rs.

use crate::domain::{cancellation::Cancellation, diagnostics::Result};
use rusqlite::{
    Connection,
    hooks::{AuthAction, AuthContext, Authorization},
};

fn legacy_shared(version: u32) -> Result<Connection> {
    let connection = super::super::tests::legacy()?;
    super::super::run(&connection, &Cancellation::default())?;
    connection.execute_batch(include_str!("../fixtures/restore_v5.sql"))?;
    let key = if version == 2 {
        "hash BLOB PRIMARY KEY"
    } else {
        "id INTEGER PRIMARY KEY,hash BLOB UNIQUE"
    };
    let layout = if version == 2 { " WITHOUT ROWID" } else { "" };
    connection.execute_batch(&format!("PRAGMA foreign_keys=OFF;
        CREATE TABLE old_data({key},lines INTEGER,first_newline INTEGER,last_newline INTEGER,content BLOB){layout};
        INSERT INTO old_data(hash,lines,first_newline,last_newline,content) SELECT d.hash,d.lines,d.first_newline,d.last_newline,kodstar_content(d.content,d.codec,d.size,b.content,b.codec,b.size) FROM chunk_data d LEFT JOIN chunk_data b ON b.id=d.base;
        CREATE TEMP TABLE refs AS SELECT r.blob,r.part,d.hash FROM chunk_refs r JOIN chunk_data d ON d.id=r.chunk;
        DROP VIEW chunks; DROP TABLE chunk_refs; DROP TABLE chunk_data;
        ALTER TABLE old_data RENAME TO chunk_data;
        CREATE TABLE chunk_refs(blob INTEGER REFERENCES blobs(id),part INTEGER,hash BLOB REFERENCES chunk_data(hash),PRIMARY KEY(blob,part)) WITHOUT ROWID;
        INSERT INTO chunk_refs SELECT * FROM refs; DROP TABLE refs;
        CREATE VIEW chunks AS SELECT r.blob,r.part,d.content FROM chunk_refs r JOIN chunk_data d ON d.hash=r.hash;
        PRAGMA user_version={version}; PRAGMA foreign_keys=ON;"))?;
    Ok(connection)
}

#[test]
fn shared_conversion_preserves_version_and_contents_on_failure() -> Result<()> {
    for (version, mode) in [2, 3]
        .into_iter()
        .flat_map(|version| ["cancel", "corrupt", "unknown", "full"].map(|mode| (version, mode)))
    {
        let connection = legacy_shared(version)?;
        let cancel = Cancellation::default();
        if mode == "cancel" {
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
        } else if mode == "corrupt" {
            connection.execute("UPDATE chunk_data SET lines=lines+1", [])?;
        } else if mode == "unknown" {
            connection.execute(
                "ALTER TABLE chunk_data ADD COLUMN private_note TEXT DEFAULT 'keep'",
                [],
            )?;
        } else {
            connection.execute_batch("VACUUM;")?;
            let pages: u64 = connection.pragma_query_value(None, "page_count", |r| r.get(0))?;
            connection.pragma_update(None, "max_page_count", pages)?;
        }
        assert!(super::super::run(&connection, &cancel).is_err());
        connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)?;
        assert_eq!(
            connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
            version
        );
        assert_eq!(
            connection.query_row("SELECT count(*) FROM chunk_data", [], |r| r
                .get::<_, u64>(0))?,
            3
        );
        assert_eq!(
            connection.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'legacy_chunk%'",
                [],
                |r| r.get::<_, u64>(0)
            )?,
            0
        );
        if mode == "unknown" {
            assert_eq!(
                connection.query_row("SELECT private_note FROM chunk_data LIMIT 1", [], |r| {
                    r.get::<_, String>(0)
                })?,
                "keep"
            );
        }
    }
    Ok(())
}

#[test]
fn shared_versions_preserve_all_original_content_on_success() -> Result<()> {
    for version in [2, 3] {
        let connection = legacy_shared(version)?;
        let mut expected = Vec::new();
        for id in [1, 2] {
            let mut select =
                connection.prepare("SELECT content FROM chunks WHERE blob=?1 ORDER BY part")?;
            let contents = select
                .query_map([id], |r| r.get::<_, Vec<u8>>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            expected.push(contents.concat());
        }
        super::super::run(&connection, &Cancellation::default())?;
        assert_eq!(
            connection.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
            6
        );
        for (index, bytes) in expected.into_iter().enumerate() {
            assert_eq!(
                crate::infrastructure::history::content::blobs::read(
                    &connection,
                    index as i64 + 1,
                    bytes.len(),
                    &Cancellation::default()
                )?,
                bytes
            );
        }
    }
    Ok(())
}

#[test]
fn upgrade_preserves_valid_unreferenced_content_addresses() -> Result<()> {
    let connection = legacy_shared(3)?;
    let content = b"retained unreferenced bytes";
    let hash = blake3::hash(content);
    connection.execute("INSERT INTO chunk_data(hash,lines,first_newline,last_newline,content) VALUES(?1,0,-1,-1,?2)",rusqlite::params![hash.as_bytes().as_slice(),content.as_slice()])?;
    super::super::run(&connection, &Cancellation::default())?;
    let id: i64 = connection.query_row(
        "SELECT id FROM chunk_data WHERE hash=?1",
        [hash.as_bytes().as_slice()],
        |r| r.get(0),
    )?;
    assert_eq!(
        crate::infrastructure::history::content::chunk_read::Reader::new(&connection).load(id)?,
        content
    );
    Ok(())
}
