// Purpose: 선택한 소비 fact의 lookup만 제거하고 같은 블록의 다른 소비자를 보존.
// Pattern: Bounded Mutation.
// Usage: 부분 해석 준비가 changed_files·affected_facts를 전달.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/mod.rs, crates/kodstar-core/src/infrastructure/index/invalidation.rs.

use super::codec;
use crate::domain::diagnostics::Result;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use std::collections::BTreeSet;

pub(crate) fn remove_selected(c: &Connection) -> Result<()> {
    c.execute_batch("DELETE FROM lookup_rows WHERE file_id IN (SELECT id FROM changed_files);
        CREATE TEMP TABLE lookup_pending_blocks(key_id INTEGER,file_id INTEGER,part INTEGER,PRIMARY KEY(key_id,file_id,part)) WITHOUT ROWID;
        INSERT INTO lookup_pending_blocks SELECT key_id,file_id,part FROM lookup_rows WHERE file_id IN (SELECT id FROM dependent_files);")?;
    let mut after = (0i64, 0i64, -1i64);
    let slots = std::iter::repeat_n("?", codec::MAX_COUNT)
        .collect::<Vec<_>>()
        .join(",");
    let mut selected = c.prepare(&format!(
        "SELECT id FROM affected_facts WHERE id IN ({slots})"
    ))?;
    let mut next_block = c.prepare(
        "SELECT p.key_id,p.file_id,p.part,b.count,b.payload FROM lookup_pending_blocks p
        JOIN lookup_rows b ON b.key_id=p.key_id AND b.file_id=p.file_id AND b.part=p.part WHERE (p.key_id,p.file_id,p.part)>(?1,?2,?3)
        ORDER BY p.key_id,p.file_id,p.part LIMIT 1",
    )?;
    let mut update = c.prepare(
        "UPDATE lookup_rows SET count=?4,payload=?5 WHERE key_id=?1 AND file_id=?2 AND part=?3",
    )?;
    let mut remove =
        c.prepare("DELETE FROM lookup_rows WHERE key_id=?1 AND file_id=?2 AND part=?3")?;
    loop {
        let next: Option<(i64, i64, i64, usize, Vec<u8>)> = next_block
            .query_row(params![after.0, after.1, after.2], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .optional()?;
        let Some((key, file, part, count, input)) = next else {
            break;
        };
        let mut postings = Vec::with_capacity(count);
        codec::visit(&input, count, |fact, found| {
            postings.push((fact, found));
            Ok(())
        })?;
        let ids = postings
            .iter()
            .map(|(id, _)| Some(*id))
            .chain(std::iter::repeat(None))
            .take(codec::MAX_COUNT);
        let removed = selected
            .query_map(params_from_iter(ids), |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;
        if removed.len() == count {
            remove.execute(params![key, file, part])?;
        } else if !removed.is_empty() {
            let (mut output, mut previous, mut retained) = (Vec::with_capacity(input.len()), 0, 0);
            for (fact, found) in postings {
                if !removed.contains(&fact) {
                    codec::push(&mut output, fact, previous, found)?;
                    previous = fact;
                    retained += 1;
                }
            }
            update.execute(params![key, file, part, retained, output])?;
        }
        after = (key, file, part);
    }
    c.execute_batch("DROP TABLE lookup_pending_blocks;")?;
    Ok(())
}
