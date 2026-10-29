// Purpose: 형식 5 metadata 쓰기 중 취소가 schema·해시·참조를 모두 되돌리는지 검증.
// Pattern: Failure Regression Test.
// Usage: metadata 전환이 실제 구형 표·청크와 update hook을 사용.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/metadata/mod.rs.

use super::*;
use crate::infrastructure::history::content::chunks;

#[test]
fn cancellation_after_metadata_writes_restores_the_entire_prior_layout() -> Result<()> {
    let c = Connection::open_in_memory()?;
    c.execute_batch(include_str!("../fixtures/schema_v5.sql"))?;
    c.execute_batch(chunks::SCHEMA)?;
    c.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA user_version=5;
        INSERT INTO checkpoints VALUES(1,NULL,0,1,1),(2,1,0,1,1);
        UPDATE branches SET checkpoint=2;",
    )?;
    for (id, raw) in [(1, b"first".as_slice()), (2, b"later".as_slice())] {
        let hash = blake3::hash(raw);
        c.execute(
            "INSERT INTO blobs VALUES(?1,?2,?3)",
            params![id, hash.to_hex().as_str(), raw.len()],
        )?;
        c.execute(
            "INSERT INTO chunk_data VALUES(?1,?2,?3,0,NULL,0,-1,-1,?4)",
            params![id, hash.as_bytes().as_slice(), raw.len(), raw],
        )?;
        c.execute("INSERT INTO chunk_refs VALUES(?1,0,0,?1)", [id])?;
        c.execute("INSERT INTO entries VALUES(?1,X'61',?1,420,'file')", [id])?;
    }
    c.execute_batch("INSERT INTO working VALUES(X'61',2,420,'file',5,'','');")?;
    let cancel = Cancellation::default();
    let signal = cancel.clone();
    c.update_hook(Some(
        move |_: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
            if table == "paths" {
                signal.cancel();
            }
        },
    ))?;
    assert!(super::super::run(&c, &cancel).is_err());
    c.update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>)?;
    assert!(c.is_autocommit());
    assert!(c.pragma_query_value::<bool, _>(None, "foreign_keys", |r| r.get(0))?);
    assert_eq!(
        c.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
        5
    );
    assert_eq!(c.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('paths','entry_rows','metadata_blobs','metadata_old_entries')",[],|r|r.get::<_,i64>(0))?,0);
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM blobs WHERE typeof(hash)='text'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        2
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM entries", [], |r| r.get::<_, i64>(0))?,
        2
    );
    super::super::run(&c, &Cancellation::default())?;
    Ok(())
}
