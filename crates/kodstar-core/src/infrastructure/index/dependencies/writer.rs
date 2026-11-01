// Purpose: 한 lookup 그룹의 기존 블록을 보존하며 변경된 정수 postings만 병합.
// Pattern: Bounded Differential Writer.
// Usage: dependency publication이 key·파일 그룹을 한 번씩 전달.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/publication.rs, crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs.

use super::codec;
use crate::domain::diagnostics::{Error, Result};
use rusqlite::{Connection, OptionalExtension, Statement, params, params_from_iter};
use std::collections::{BTreeMap, BTreeSet};

type Posting = (i64, bool);
type Block = (i64, usize, Vec<u8>);

pub(super) struct Writer<'a> {
    c: &'a Connection,
    replacements: Statement<'a>,
    selected: Option<Statement<'a>>,
    selected_file: Option<Statement<'a>>,
    pending: Statement<'a>,
    consume: Statement<'a>,
    consume_prefix: Statement<'a>,
    next: Statement<'a>,
    store: Statement<'a>,
    remove: Statement<'a>,
}

impl<'a> Writer<'a> {
    pub fn new(c: &'a Connection, partial: bool) -> Result<Self> {
        let slots = std::iter::repeat_n("?", codec::MAX_COUNT)
            .collect::<Vec<_>>()
            .join(",");
        Ok(Self {
            c,
            replacements: c.prepare(&format!("SELECT fact_id,found FROM dependency_rows
                WHERE key_id=? AND file_id=? AND fact_id IN ({slots})"))?,
            selected: partial.then(|| c.prepare(&format!("SELECT id FROM affected_facts WHERE id IN ({slots})"))).transpose()?,
            selected_file: partial.then(|| c.prepare("SELECT EXISTS(SELECT 1 FROM changed_files WHERE id=?1) AND NOT EXISTS(SELECT 1 FROM body_files WHERE id=?1)")).transpose()?,
            pending: c.prepare("SELECT fact_id,found FROM dependency_rows WHERE key_id=?1 AND file_id=?2 ORDER BY fact_id LIMIT ?3")?,
            consume: c.prepare("DELETE FROM dependency_rows WHERE key_id=?1 AND file_id=?2 AND fact_id=?3")?,
            consume_prefix: c.prepare("DELETE FROM dependency_rows WHERE key_id=?1 AND file_id=?2 AND fact_id<=?3")?,
            next: c.prepare("SELECT part,count,payload FROM lookup_rows WHERE key_id=?1 AND file_id=?2
                AND part>?3 AND (?4=0 OR count<128) ORDER BY part LIMIT 1")?,
            store: c.prepare("INSERT INTO lookup_rows(key_id,file_id,part,count,payload) VALUES(?1,?2,?3,?4,?5)
                ON CONFLICT(key_id,file_id,part) DO UPDATE SET count=excluded.count,payload=excluded.payload
                WHERE lookup_rows.count<>excluded.count OR lookup_rows.payload<>excluded.payload")?,
            remove: c.prepare("DELETE FROM lookup_rows WHERE key_id=?1 AND file_id=?2 AND part=?3")?,
        })
    }

    pub fn merge(&mut self, key: i64, file: i64) -> Result<()> {
        let whole_file = self
            .selected_file
            .as_mut()
            .map(|q| q.query_row([file], |r| r.get::<_, bool>(0)))
            .transpose()?
            .unwrap_or(false);
        let mut part = -1;
        while let Some((id, count, payload)) = self.block(key, file, part, false)? {
            let old = decode(&payload, count)?;
            let ids = || {
                old.iter()
                    .map(|(id, _)| Some(*id))
                    .chain(std::iter::repeat(None))
                    .take(codec::MAX_COUNT)
            };
            let values = [Some(key), Some(file)].into_iter().chain(ids());
            let mut replacements: BTreeMap<i64, bool> = self
                .replacements
                .query_map(params_from_iter(values), |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let selected: BTreeSet<i64> = if let Some(q) = &mut self.selected {
                q.query_map(params_from_iter(ids()), |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            } else {
                BTreeSet::new()
            };
            let mut retained = Vec::with_capacity(old.len());
            for (fact, found) in old {
                if let Some(updated) = replacements.remove(&fact) {
                    retained.push((fact, updated));
                    self.consume.execute(params![key, file, fact])?;
                } else if !whole_file && !selected.contains(&fact) {
                    retained.push((fact, found));
                }
            }
            self.write(key, file, id, &retained)?;
            part = id;
        }
        // Old IDs have all been consumed, so filling holes cannot duplicate a later block.
        let mut after = -1;
        while self.has_pending(key, file)? {
            let Some((id, count, payload)) = self.block(key, file, after, true)? else {
                break;
            };
            let mut rows = decode(&payload, count)?;
            rows.extend(self.take(key, file, codec::MAX_COUNT - count)?);
            rows.sort_unstable_by_key(|row| row.0);
            self.write(key, file, id, &rows)?;
            after = id;
        }
        while self.has_pending(key, file)? {
            part = part
                .checked_add(1)
                .ok_or_else(|| Error::operation("lookup block sequence overflow"))?;
            let rows = self.take(key, file, codec::MAX_COUNT)?;
            self.write(key, file, part, &rows)?;
        }
        Ok(())
    }

    fn block(&mut self, key: i64, file: i64, after: i64, vacant: bool) -> Result<Option<Block>> {
        Ok(self
            .next
            .query_row(params![key, file, after, vacant], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?)
    }

    fn has_pending(&self, key: i64, file: i64) -> Result<bool> {
        Ok(self.c.query_row(
            "SELECT EXISTS(SELECT 1 FROM dependency_rows WHERE key_id=?1 AND file_id=?2)",
            params![key, file],
            |r| r.get(0),
        )?)
    }

    fn take(&mut self, key: i64, file: i64, limit: usize) -> Result<Vec<Posting>> {
        let rows: Vec<Posting> = self
            .pending
            .query_map(params![key, file, limit], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        if let Some((last, _)) = rows.last() {
            self.consume_prefix.execute(params![key, file, last])?;
        }
        Ok(rows)
    }

    fn write(&mut self, key: i64, file: i64, part: i64, rows: &[Posting]) -> Result<()> {
        if rows.is_empty() {
            self.remove.execute(params![key, file, part])?;
        } else {
            let mut bytes = Vec::with_capacity(rows.len() * 2);
            let mut previous = 0;
            for &(id, found) in rows {
                codec::push(&mut bytes, id, previous, found)?;
                previous = id;
            }
            self.store
                .execute(params![key, file, part, rows.len(), bytes])?;
        }
        Ok(())
    }
}

fn decode(bytes: &[u8], count: usize) -> Result<Vec<Posting>> {
    let mut rows = Vec::with_capacity(count.min(codec::MAX_COUNT));
    codec::visit(bytes, count, |id, found| {
        rows.push((id, found));
        Ok(())
    })?;
    Ok(rows)
}
