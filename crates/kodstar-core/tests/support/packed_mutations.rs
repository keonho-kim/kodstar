// Purpose: 내용 의미를 유지한 물리 ID·scope 변경으로 증분 보존 검증.
// Pattern: Test Fixture Mutation.
// Usage: 증분 회귀 사례가 테스트 소유 인덱스만 변경.
// Related: crates/kodstar-core/tests/support/packed.rs.

use rusqlite::{Connection, Result, params};

fn integer(bytes: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        bytes.push(value as u8 | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
}

pub fn mark(c: &Connection, file: &str) -> Result<()> {
    super::postings::display_path(c)?;
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
