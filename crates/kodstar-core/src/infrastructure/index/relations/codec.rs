// Purpose: 같은 source의 관계 ID·fact 차이·nullable 대상·해석 상태 인코딩.
// Pattern: Adjacency Block Encoding.
// Usage: relations writer와 packed cursor가 최대 128개 관계를 교환.
// Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs, crates/kodstar-core/src/infrastructure/index/packed/codec.rs.

use super::super::packed::{
    Row,
    codec::{self, Input},
};
use rusqlite::{Result, types::Value};

const STATES: [&str; 4] = ["resolved", "candidate", "ambiguous", "unresolved"];

pub(super) fn id(row: &Row, column: usize) -> Result<i64> {
    match row[column] {
        Value::Integer(n) if n > 0 => Ok(n),
        _ => Err(codec::invalid()),
    }
}

pub(super) fn encode(rows: &[Row]) -> Result<Vec<u8>> {
    let (mut bytes, mut previous, mut fact) = (Vec::new(), 0i64, 0i64);
    for row in rows {
        let current = id(row, 0)?;
        let next = id(row, 1)?;
        if current <= previous {
            return Err(codec::invalid());
        }
        codec::integer(&mut bytes, (current - previous) as u64);
        let delta = next - fact;
        codec::integer(&mut bytes, ((delta << 1) ^ (delta >> 63)) as u64);
        for column in [3, 6, 7] {
            codec::integer(
                &mut bytes,
                match row[column] {
                    Value::Null if column != 6 => 0,
                    _ => id(row, column)? as u64,
                },
            );
        }
        let Value::Text(state) = &row[5] else {
            return Err(codec::invalid());
        };
        let state = STATES
            .iter()
            .position(|s| s == state)
            .ok_or_else(codec::invalid)?;
        let external = match &row[4] {
            Value::Null => None,
            Value::Text(s) => Some(s.as_str()),
            _ => return Err(codec::invalid()),
        };
        if row[3] != Value::Null && external.is_some()
            || state == 0 && row[3] == Value::Null && external.is_none()
        {
            return Err(codec::invalid());
        }
        codec::integer(
            &mut bytes,
            (state * 2 + usize::from(external.is_some())) as u64,
        );
        if let Some(text) = external {
            codec::text(&mut bytes, text);
        }
        previous = current;
        fact = next;
    }
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8], count: usize, source: i64) -> Result<Vec<Row>> {
    if !(1..=128).contains(&count) || source <= 0 {
        return Err(codec::invalid());
    }
    let mut input = Input(bytes);
    let (mut rows, mut previous, mut fact) = (Vec::with_capacity(count), 0i64, 0i64);
    for _ in 0..count {
        let gap = input.id()?;
        let id = previous.checked_add(gap).ok_or_else(codec::invalid)?;
        let delta = input.integer()?;
        let delta = ((delta >> 1) as i64) ^ -((delta & 1) as i64);
        fact = fact.checked_add(delta).ok_or_else(codec::invalid)?;
        let target = input.id()?;
        let rule = input.id()?;
        let reason = input.id()?;
        let flags = input.integer()?;
        if gap == 0 || fact <= 0 || rule == 0 || flags > 7 {
            return Err(codec::invalid());
        }
        let external = if flags & 1 == 1 {
            Value::Text(input.text()?)
        } else {
            Value::Null
        };
        if target > 0 && external != Value::Null
            || flags >> 1 == 0 && target == 0 && external == Value::Null
        {
            return Err(codec::invalid());
        }
        rows.push(vec![
            Value::Integer(id),
            Value::Integer(fact),
            Value::Integer(source),
            nullable(target),
            external,
            Value::Text(STATES[(flags >> 1) as usize].into()),
            Value::Integer(rule),
            nullable(reason),
        ]);
        previous = id;
    }
    input.finish()?;
    Ok(rows)
}

fn nullable(id: i64) -> Value {
    if id == 0 {
        Value::Null
    } else {
        Value::Integer(id)
    }
}
