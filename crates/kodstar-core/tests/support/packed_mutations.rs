// Purpose: 물리 ID·scope 표시 변경과 관계 ID 조회로 증분 보존 관측.
// Pattern: Test Fixture Mutation.
// Usage: 증분 회귀 사례가 테스트 소유 인덱스를 표시하고 갱신 후 ID를 대조.
// Related: crates/kodstar-core/tests/support/packed.rs, crates/kodstar-core/tests/support/postings.rs, crates/kodstar-core/tests/cases/incremental_bodies.rs, crates/kodstar-core/tests/cases/optimization.rs.

use kodstar_core::InitReport;
use rusqlite::{Connection, Result, params};

pub fn relationship_ids(report: &InitReport, file: &str) -> Result<Vec<i64>> {
    let connection = Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    read_relationship_ids(&connection, file)
}

pub fn mark_relationship_ids(report: &InitReport, file: &str) -> Result<Vec<i64>> {
    let connection = Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    mark(&connection, file)?;
    let ids = read_relationship_ids(&connection, file)?;
    assert!(
        !ids.is_empty(),
        "fixture has no relationships to preserve: {file}"
    );
    Ok(ids)
}

fn read_relationship_ids(connection: &Connection, file: &str) -> Result<Vec<i64>> {
    super::postings::register(connection)?;
    let mut statement = connection.prepare(
        "SELECT r.id FROM relationships r JOIN facts u ON u.id=r.fact_id
         JOIN files f ON f.id=u.file_id WHERE f.path=?1 ORDER BY r.id",
    )?;
    statement
        .query_map([file.as_bytes()], |row| row.get(0))?
        .collect()
}

fn integer(bytes: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        bytes.push(value as u8 | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
}

pub fn mark(c: &Connection, file: &str) -> Result<()> {
    super::postings::register(c)?;
    let blocks=c.prepare("SELECT b.id,b.payload FROM relation_blocks b JOIN files f ON f.id=b.file_id WHERE f.display_path=?1")?.query_map([file],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Vec<u8>>(1)?)))?.collect::<Result<Vec<_>>>()?;
    let tx = c.unchecked_transaction()?;
    for (block, bytes) in blocks {
        let mut rest = bytes.as_slice();
        let first = super::packed::number(&mut rest)?;
        let mut output = Vec::new();
        integer(&mut output, first + 1_000_000);
        output.extend_from_slice(rest);
        tx.execute(
            "UPDATE relation_id_ranges SET first=first+1000000,last=last+1000000 WHERE block=?1",
            [block],
        )?;
        tx.execute(
            "UPDATE relation_blocks SET payload=?2 WHERE id=?1",
            params![block, output],
        )?;
    }
    tx.commit()
}

pub fn remap_scopes(c: &Connection) -> Result<()> {
    let blocks = c
        .prepare("SELECT id,count,payload FROM dictionary_blocks WHERE kind LIKE 'scope:%'")?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, usize>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>>>()?;
    for (block, count, bytes) in blocks {
        let mut keys = super::packed::keys(&bytes, count)?;
        for (_, key) in &mut keys {
            let (file, rest) = key.split_once(':').ok_or(rusqlite::Error::InvalidQuery)?;
            let file = file
                .parse::<u64>()
                .map_err(|_| rusqlite::Error::InvalidQuery)?
                + 1000;
            *key = format!("{file}:{rest}");
        }
        keys.sort_by(|a, b| a.1.cmp(&b.1));
        let mut output = Vec::new();
        let mut previous = "";
        for (id, key) in &keys {
            let shared = previous
                .bytes()
                .zip(key.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            integer(&mut output, *id);
            integer(&mut output, shared as u64);
            integer(&mut output, (key.len() - shared) as u64);
            output.extend_from_slice(&key.as_bytes()[shared..]);
            previous = key;
        }
        c.execute(
            "UPDATE dictionary_blocks SET first=?2,last=?3,payload=?4 WHERE id=?1",
            params![block, keys[0].1, keys[keys.len() - 1].1, output],
        )?;
    }
    Ok(())
}
