// Purpose: 압축 lookup의 독립 기준 decoder를 통합 테스트 SQL에 제공.
// Pattern: Reference Decoder.
// Usage: snapshot과 publication rollback 사례가 연결에 등록.
// Related: scripts/benchmarks/postings.py, crates/kodstar-core/tests/support/snapshot.rs.

#[path = "lookup_pages.rs"]
pub mod lookup_pages;

pub fn register(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    super::packed::register(connection)?;
    display_path(connection)?;
    connection.create_scalar_function(
        "kodstar_digest",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            use rusqlite::types::ValueRef;
            match ctx.get_raw(0) {
                ValueRef::Null => Ok(None),
                ValueRef::Blob(v) if v.len() == 32 => Ok(Some(v.to_vec())),
                ValueRef::Text(v) => {
                    let text = std::str::from_utf8(v).map_err(|_| rusqlite::Error::InvalidQuery)?;
                    let bytes =
                        blake3::Hash::from_hex(text).map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(Some(bytes.as_bytes().to_vec()))
                }
                _ => Err(rusqlite::Error::InvalidQuery),
            }
        },
    )?;
    super::packed::expansion::register(connection, c"kodstar_posting_rows", |bytes, count| {
        let count = usize::try_from(count).map_err(|_| rusqlite::Error::InvalidQuery)?;
        if !(1..=128).contains(&count) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut bytes = bytes;
        let mut pairs = Vec::new();
        let mut previous = 0i64;
        for _ in 0..count {
            let value = super::packed::number(&mut bytes)?;
            let next = i64::try_from(value >> 1)
                .ok()
                .and_then(|gap| previous.checked_add(gap))
                .filter(|next| *next > previous)
                .ok_or(rusqlite::Error::InvalidQuery)?;
            pairs.push([next, (value & 1) as i64]);
            previous = next;
        }
        if !bytes.is_empty() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(pairs)
    })?;
    lookup_pages::register(connection)
}

pub fn display_path(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    connection.create_scalar_function(
        "kodstar_display_path",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |context| Ok(String::from_utf8_lossy(context.get_raw(0).as_blob()?).into_owned()),
    )
}
