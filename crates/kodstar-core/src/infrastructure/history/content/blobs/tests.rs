// Purpose: 중복 저장과 순차 입력·취소·손상의 원자성 검증.
// Pattern: Unit Test.
// Usage: history blob adapter의 고정 버퍼 계약 회귀 검사.
// Related: crates/kodstar-core/src/infrastructure/history/content/blobs.rs.

use super::*;
use std::io::{Cursor, Read};

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
    let first =
        Writer::new(&connection)?.put(Cursor::new(&bytes), bytes.len() as u64, None, &cancel)?;
    let pages: u64 = connection.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    bytes[CHUNK + 13] ^= 1;
    let second =
        Writer::new(&connection)?.put(Cursor::new(&bytes), bytes.len() as u64, None, &cancel)?;
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
    let id =
        Writer::new(&connection)?.put(Cursor::new(&bytes), bytes.len() as u64, None, &cancel)?;
    let pages: u64 = connection.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let writes = connection.total_changes();
    let transaction = connection.transaction()?;
    assert_eq!(
        Writer::new(&transaction)?.put(Cursor::new(&bytes), bytes.len() as u64, None, &cancel)?,
        id
    );
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

struct SequentialInput {
    bytes: Cursor<Vec<u8>>,
    cancellation: Option<Cancellation>,
}

impl Read for SequentialInput {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let count = self.bytes.read(output)?;
        if self.bytes.position() > CHUNK as u64
            && let Some(cancel) = &self.cancellation
        {
            cancel.cancel();
        }
        Ok(count)
    }
}

#[test]
fn truncated_or_cancelled_input_rolls_back_new_contents() -> Result<()> {
    for change in [true, false] {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../../schema.sql"))?;
        connection.execute_batch(crate::infrastructure::history::content::chunks::SCHEMA)?;
        super::super::chunks::register(&connection)?;
        let cancel = Cancellation::default();
        let input = SequentialInput {
            bytes: Cursor::new(vec![9; CHUNK * 2]),
            cancellation: (!change).then(|| cancel.clone()),
        };
        let transaction = connection.transaction()?;
        let error = Writer::new(&transaction)?
            .put(
                input,
                (CHUNK * 2 + usize::from(change)) as u64,
                None,
                &cancel,
            )
            .expect_err("unstable input was published");
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
fn one_pass_input_round_trips_large_contents() -> Result<()> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(include_str!("../../schema.sql"))?;
    connection.execute_batch(crate::infrastructure::history::content::chunks::SCHEMA)?;
    let data = vec![3; CHUNK * 2 + 1];
    let size = data.len();
    let cancel = Cancellation::default();
    let source = SequentialInput {
        bytes: Cursor::new(data.clone()),
        cancellation: None,
    };
    let id = Writer::new(&connection)?.put(source, size as u64, None, &cancel)?;
    assert_eq!(read(&connection, id, size, &cancel)?, data);
    Ok(())
}

#[test]
fn corrupted_length_never_writes_beyond_the_declared_size() -> Result<()> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(include_str!("../../schema.sql"))?;
    connection.execute_batch(crate::infrastructure::history::content::chunks::SCHEMA)?;
    super::super::chunks::register(&connection)?;
    let cancel = Cancellation::default();
    let id =
        Writer::new(&connection)?.put(Cursor::new(vec![7; CHUNK]), CHUNK as u64, None, &cancel)?;
    connection.execute("UPDATE blobs SET size=1 WHERE id=?1", [id])?;
    let mut output = Vec::new();
    assert!(write(&connection, id, &mut output, &cancel).is_err());
    assert!(
        output.is_empty(),
        "corrupt input escaped the declared byte bound"
    );
    Ok(())
}
