// Purpose: 출처·관계 ID와 독립적인 후보 정보의 순서 보존 인코딩.
// Pattern: Bounded Encoding.
// Usage: 관계 codec이 후보 그룹을 저장하고 decoder가 인라인·공유 정보를 복원.
// Related: crates/kodstar-core/src/infrastructure/index/relations/codec.rs, crates/kodstar-core/src/infrastructure/index/relations/decode.rs.

use super::super::Relation;
use crate::domain::relations::Resolution;
use crate::infrastructure::index::packed::codec::{self, Input};
use rusqlite::Result;

pub(in crate::infrastructure::index::relations) struct Target<'a> {
    pub target: Option<i64>,
    pub external: Option<&'a str>,
    pub state: Resolution,
    pub rule: i64,
    pub reason: Option<i64>,
}

pub(in crate::infrastructure::index::relations) fn encode(rows: &[Relation]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for row in rows {
        row.validate()?;
        for id in [row.target, Some(row.rule), row.reason] {
            codec::integer(&mut bytes, id.unwrap_or(0) as u64);
        }
        let state = match row.state {
            Resolution::Resolved => 0,
            Resolution::Candidate => 1,
            Resolution::Ambiguous => 2,
            Resolution::Unresolved => 3,
        };
        codec::integer(&mut bytes, state * 2 + u64::from(row.external.is_some()));
        if let Some(text) = &row.external {
            codec::text(&mut bytes, text);
        }
    }
    Ok(bytes)
}

pub(in crate::infrastructure::index::relations) fn read<'a>(
    input: &mut Input<'a>,
) -> Result<Target<'a>> {
    let target = input.id()?;
    let rule = input.id()?;
    let reason = input.id()?;
    let flags = input.integer()?;
    if rule == 0 || flags > 7 {
        return Err(codec::invalid());
    }
    let external = if flags & 1 == 1 {
        Some(input.text_ref()?)
    } else {
        None
    };
    let state = match flags >> 1 {
        0 => Resolution::Resolved,
        1 => Resolution::Candidate,
        2 => Resolution::Ambiguous,
        _ => Resolution::Unresolved,
    };
    if target > 0 && external.is_some()
        || state == Resolution::Resolved && target == 0 && external.is_none()
    {
        return Err(codec::invalid());
    }
    Ok(Target {
        target: (target > 0).then_some(target),
        external,
        state,
        rule,
        reason: (reason > 0).then_some(reason),
    })
}
