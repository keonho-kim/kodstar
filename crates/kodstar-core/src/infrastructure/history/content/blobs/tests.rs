// Purpose: 중복 저장의 쓰기 억제와 재읽기·취소·손상 입력의 원자성 검증.
// Pattern: Unit Test.
// Usage: history blob adapter의 고정 버퍼 계약 회귀 검사.
// Related: crates/kodstar-core/src/infrastructure/history/content/blobs.rs.

use super::*;
use std::io::{Cursor, Seek, SeekFrom};

#[test]
fn small_edits_share_unchanged_chunks_without_losing_content() -> Result<()> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(include_str!("../../schema.sql"))?;
    connection.execute_batch(crate::infrastructure::history::content::chunks::SCHEMA)?;
    super::super::chunks::register(&connection)?;
    let cancel = Cancellation::default();
    let mut bytes = Vec::new();
    for value in 0..4 {
        bytes.extend(vec![value; CHUNK]);
    }
    let first = put(&connection, Cursor::new(&bytes), None, &cancel)?;
    let pages: u64 = connection.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    bytes[CHUNK + 13] ^= 1;
    let second = put(&connection, Cursor::new(&bytes), None, &cancel)?;
    let after: u64 = connection.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let page_size: u64 = connection.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    assert!(
        (after - pages) * page_size < 2 * CHUNK as u64,
        "unchanged chunks were duplicated"
    );
    assert_eq!(read(&connection, second, bytes.len(), &cancel)?, bytes);
    bytes[CHUNK + 13] ^= 1;
    assert_eq!(read(&connection, first, bytes.len(), &cancel)?, bytes);
    Ok(())
}

#[test]
fn duplicate_contents_do_not_write_or_allocate_database_pages() -> Result<()> {
    let mut connection = Connection::open_in_memory()?;
    connection.execute_batch(include_str!("../../schema.sql"))?;
    connection.execute_batch(crate::infrastructure::history::content::chunks::SCHEMA)?;
    super::super::chunks::register(&connection)?;
    let cancel = Cancellation::default();
    let bytes = vec![7; CHUNK * 4 + 1];
    let id = put(&connection, Cursor::new(&bytes), None, &cancel)?;
    let pages: u64 = connection.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let writes = connection.total_changes();
    let transaction = connection.transaction()?;
    assert_eq!(put(&transaction, Cursor::new(&bytes), None, &cancel)?, id);
    transaction.commit()?;
    assert_eq!(
        connection.total_changes(),
        writes,
        "duplicate source rewrote pages"
    );
    let after: u64 = connection.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    assert_eq!(after, pages);
    Ok(())
}

enum OnRewind {
    Change,
    Cancel(Cancellation),
}

struct ReopenedInput {
    bytes: Cursor<Vec<u8>>,
    action: OnRewind,
}

impl Read for ReopenedInput {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        self.bytes.read(output)
    }
}

impl Seek for ReopenedInput {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        if matches!(position, SeekFrom::Start(_)) {
            match &self.action {
                OnRewind::Change => self.bytes.get_mut()[0] ^= 1,
                OnRewind::Cancel(cancel) => cancel.cancel(),
            }
        }
        self.bytes.seek(position)
    }
}

#[test]
fn changed_or_cancelled_reread_rolls_back_new_contents() -> Result<()> {
    for change in [true, false] {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../../schema.sql"))?;
        connection.execute_batch(crate::infrastructure::history::content::chunks::SCHEMA)?;
        super::super::chunks::register(&connection)?;
        let cancel = Cancellation::default();
        let input = ReopenedInput {
            bytes: Cursor::new(vec![9; CHUNK * 2]),
            action: if change {
                OnRewind::Change
            } else {
                OnRewind::Cancel(cancel.clone())
            },
        };
        let transaction = connection.transaction()?;
        let error =
            put(&transaction, input, None, &cancel).expect_err("unstable input was published");
        if change {
            assert!(
                error.to_string().contains("changed during capture"),
                "{error}"
            );
        } else {
            assert_eq!(error.kind, crate::domain::diagnostics::ErrorKind::Cancelled);
        }
        transaction.rollback()?;
        let count: u64 = connection.query_row(
            "SELECT (SELECT count(*) FROM blobs)+(SELECT count(*) FROM chunks)",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 0);
    }
    Ok(())
}

#[test]
fn corrupted_length_never_writes_beyond_the_declared_size() -> Result<()> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(include_str!("../../schema.sql"))?;
    connection.execute_batch(crate::infrastructure::history::content::chunks::SCHEMA)?;
    super::super::chunks::register(&connection)?;
    let cancel = Cancellation::default();
    let id = put(&connection, Cursor::new(vec![7; CHUNK]), None, &cancel)?;
    connection.execute("UPDATE blobs SET size=1 WHERE id=?1", [id])?;
    let mut output = Vec::new();
    assert!(write(&connection, id, &mut output, &cancel).is_err());
    assert!(
        output.is_empty(),
        "corrupt input escaped the declared byte bound"
    );
    Ok(())
}
