// Purpose: 통합 검증용 lookup 페이지의 독립 복원과 명시적 손상 fixture 변경.
// Pattern: Reference Decoder.
// Usage: postings 지원이 quiescent 물리 페이지를 읽고 증분 입력 사례가 손상시킴.
// Related: crates/kodstar-core/tests/support/postings.rs, crates/kodstar-core/tests/cases/incremental_inputs.rs.

use super::super::packed::number;
use rusqlite::{Connection, Result, functions::FunctionFlags, params};

struct Row {
    slot: u64,
    key: u64,
    part: u64,
    count: u64,
    payload: Vec<u8>,
}

fn decode(mut bytes: &[u8], count: usize) -> Result<Vec<Row>> {
    if count == 0 || count > 64 || bytes.len() > 65536 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut rows = Vec::new();
    for _ in 0..count {
        let (slot, key, part, count, size) = (
            number(&mut bytes)?,
            number(&mut bytes)?,
            number(&mut bytes)?,
            number(&mut bytes)?,
            number(&mut bytes)? as usize,
        );
        if slot >= 64 || !(1..=128).contains(&key) || !(1..=128).contains(&count) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let payload = bytes
            .get(..size)
            .ok_or(rusqlite::Error::InvalidQuery)?
            .to_vec();
        bytes = &bytes[size..];
        rows.push(Row {
            slot,
            key,
            part,
            count,
            payload,
        });
    }
    if !bytes.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(rows)
}

pub(super) fn register(c: &Connection) -> Result<()> {
    if !c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='lookup_pages')",
        [],
        |r| r.get::<_, bool>(0),
    )? {
        return Ok(());
    }
    c.create_scalar_function(
        "reference_lookup_page",
        2,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |ctx| {
            let rows = decode(ctx.get_raw(0).as_blob()?, ctx.get(1)?)?;
            let values = rows
                .into_iter()
                .map(|r| {
                    serde_json::json!([
                        r.key,
                        r.part,
                        r.count,
                        r.payload
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    ])
                })
                .collect::<Vec<_>>();
            serde_json::to_string(&values)
                .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
        },
    )?;
    c.execute_batch("CREATE TEMP VIEW IF NOT EXISTS lookup_rows AS SELECT p.group_id route_group,p.group_id*128+json_extract(j.value,'$[0]') key_id,p.file_id,
        json_extract(j.value,'$[1]') part,json_extract(j.value,'$[2]') count,unhex(json_extract(j.value,'$[3]')) payload
        FROM lookup_pages p,json_each(reference_lookup_page(p.payload,p.count)) j;
        CREATE TEMP VIEW IF NOT EXISTS lookup_blocks AS SELECT key_id,file_id,part,count,payload,route_group FROM lookup_rows;
        CREATE TEMP VIEW IF NOT EXISTS fact_lookups AS SELECT j.first fact_id,b.key_id,j.second found
        FROM lookup_blocks b,kodstar_posting_rows(b.payload,b.count) j;")
}

fn integer(bytes: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        bytes.push(n as u8 | 128);
        n >>= 7;
    }
    bytes.push(n as u8);
}

pub fn change_first(c: &Connection, replacement: Option<&[u8]>) -> Result<()> {
    let (id, count, bytes): (i64, usize, Vec<u8>) = c.query_row(
        "SELECT id,count,payload FROM lookup_pages ORDER BY id LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut rows = decode(&bytes, count)?;
    if let Some(payload) = replacement {
        rows[0].payload = payload.to_vec();
        rows[0].count = 1;
    } else {
        rows[0].payload[0] ^= 1;
    }
    let mut output = Vec::new();
    for row in rows {
        for value in [
            row.slot,
            row.key,
            row.part,
            row.count,
            row.payload.len() as u64,
        ] {
            integer(&mut output, value);
        }
        output.extend(row.payload);
    }
    c.execute(
        "UPDATE lookup_pages SET payload=?2 WHERE id=?1",
        params![id, output],
    )?;
    Ok(())
}
