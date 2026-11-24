// Purpose: 계층 정점의 정렬 키를 배치 조회하고 같은 파일의 경로를 공유.
// Pattern: Bounded Transformation.
// Usage: memory 탐색이 깊이 전환 때 호출하고 키를 즉시 해제.
// Related: crates/kodstar-core/src/application/query/traversal/memory.rs, crates/kodstar-core/src/application/query/traversal/order.sql.

use super::graph::Node;
use crate::{
    domain::{cancellation::Cancellation, diagnostics::Result},
    infrastructure::index::cached::Sql,
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

struct Key {
    node: Node,
    ordinal: usize,
    path: Arc<[u8]>,
    line: i64,
    column: i64,
    name: String,
}

pub(super) fn sort(
    sql: &Sql<'_>,
    queue: &mut VecDeque<Node>,
    bytes: usize,
    cancel: &Cancellation,
) -> Result<bool> {
    if queue.is_empty() {
        return Ok(true);
    }
    if queue.len() == 1 {
        cancel.check()?;
        // A singleton is already ordered. Validate its projection without owning
        // a path/name or constructing the multi-row input relation.
        let mut read = sql.cached_prepare(
            "SELECT path,line,column_byte,qualified_name FROM query_nodes WHERE id=?1",
        )?;
        read.query_row([queue[0].id], |row| {
            row.get_ref(0)?.as_blob()?;
            row.get::<_, i64>(1)?;
            row.get::<_, i64>(2)?;
            row.get_ref(3)?.as_str()?;
            Ok(())
        })?;
        return Ok(true);
    }
    let mut used = queue
        .len()
        .saturating_mul(std::mem::size_of::<Key>())
        .saturating_add(std::mem::size_of::<[Option<i64>; 32]>());
    if used > bytes {
        return Ok(false);
    }
    let mut keys = Vec::new();
    if keys.try_reserve_exact(queue.len()).is_err() {
        return Ok(false);
    }
    used += (keys.capacity() - queue.len()) * std::mem::size_of::<Key>();
    let mut paths = BTreeMap::<i64, Arc<[u8]>>::new();
    let mut read = sql.cached_prepare(include_str!("order.sql"))?;
    for offset in (0..queue.len()).step_by(32) {
        cancel.check()?;
        let ids = std::array::from_fn::<_, 32, _>(|i| queue.get(offset + i).map(|n| n.id));
        let mut rows = read.query(rusqlite::params_from_iter(ids))?;
        let expected = (queue.len() - offset).min(32);
        let mut count = 0;
        while let Some(r) = rows.next()? {
            cancel.check()?;
            let ordinal: usize = r.get(0)?;
            let file: i64 = r.get(1)?;
            let path = r.get_ref(2)?.as_blob().map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Blob,
                    Box::new(e),
                )
            })?;
            let name = r.get_ref(5)?.as_str().map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let path_bytes = if paths.contains_key(&file) {
                0
            } else {
                // Charge conservatively for the shared allocation and tree slots.
                path.len() + 512
            };
            let next = used.saturating_add(path_bytes).saturating_add(name.len());
            if next > bytes {
                return Ok(false);
            }
            used = next;
            let path = Arc::clone(paths.entry(file).or_insert_with(|| Arc::from(path)));
            keys.push(Key {
                node: queue[offset + ordinal],
                ordinal: offset + ordinal,
                path,
                line: r.get(3)?,
                column: r.get(4)?,
                name: name.to_owned(),
            });
            count += 1;
        }
        if count != expected {
            return Err(rusqlite::Error::QueryReturnedNoRows.into());
        }
    }
    keys.sort_unstable_by(|a, b| {
        (
            &a.path,
            a.line,
            a.column,
            &a.name,
            a.node.direction,
            a.ordinal,
        )
            .cmp(&(
                &b.path,
                b.line,
                b.column,
                &b.name,
                b.node.direction,
                b.ordinal,
            ))
    });
    queue.clear();
    queue.extend(keys.into_iter().map(|key| key.node));
    Ok(true)
}
