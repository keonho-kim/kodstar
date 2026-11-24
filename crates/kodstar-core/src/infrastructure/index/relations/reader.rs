// Purpose: snapshot의 관계 블록과 설명 문자열을 제한된 바이트 안에서 재사용.
// Pattern: Bounded Reader.
// Usage: query/relationship_rows가 최종 선택된 관계를 읽음.
// Related: crates/kodstar-core/src/infrastructure/index/relations/codec.rs, crates/kodstar-core/src/application/query/relationship_rows.rs.

use super::{Relation, decode};
use rusqlite::{Connection, Result, Statement, TransactionState, params, types::ValueRef};

#[cfg(test)]
mod tests;

pub(crate) struct Reader<'conn> {
    connection: &'conn Connection,
    address: Statement<'conn>,
    block: Statement<'conn>,
    text: Statement<'conn>,
    blocks: Vec<(Vec<Relation>, usize)>,
    texts: Vec<(i64, String)>,
    bytes: usize,
    limit: usize,
    changes: u64,
}

impl<'conn> Reader<'conn> {
    pub fn new(connection: &'conn Connection) -> Result<Self> {
        let limit = (connection.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize / 32)
            .min(128 * 1024);
        Ok(Self {
            connection,
            address: connection.prepare("SELECT block FROM relation_id_ranges WHERE first=(SELECT first FROM relation_id_ranges WHERE first<=?1 ORDER BY first DESC LIMIT 1) AND last>=?1")?,
            block: connection.prepare("SELECT source_id,count,payload FROM relation_blocks WHERE id=?1")?,
            text: connection.prepare("SELECT value FROM relation_texts WHERE id=?1")?,
            blocks: Vec::new(),
            texts: Vec::new(),
            bytes: 0,
            limit,
            changes: connection.total_changes(),
        })
    }

    fn reusable(&mut self) -> Result<bool> {
        let changes = self.connection.total_changes();
        // A write transaction can roll back without advancing total_changes again.
        // Never retain uncommitted values or reuse across autocommit snapshots.
        let stable = !self.connection.is_autocommit()
            && self.connection.transaction_state(Some(rusqlite::MAIN_DB))?
                == TransactionState::Read;
        if !stable || changes != self.changes {
            self.blocks.clear();
            self.texts.clear();
            self.bytes = 0;
            self.changes = changes;
        }
        Ok(stable)
    }

    pub fn read(&mut self, id: i64) -> Result<Relation> {
        let reusable = self.reusable()?;
        for slot in 0..self.blocks.len() {
            if let Ok(at) = self.blocks[slot].0.binary_search_by_key(&id, |row| row.id) {
                let row = self.blocks[slot].0[at].clone();
                self.blocks[..=slot].rotate_right(1);
                return Ok(row);
            }
        }
        let block: i64 = self.address.query_row([id], |r| r.get(0))?;
        let (row, retained, bytes) = self.block.query_row([block], |r| {
            read_block(
                self.connection,
                r.get_ref(2)?.as_blob()?,
                r.get(1)?,
                r.get(0)?,
                id,
                if reusable { self.limit / 2 } else { 0 },
            )
        })?;
        let Some(rows) = retained else {
            return Ok(row);
        };
        while self.blocks.len() == 4 || self.bytes.saturating_add(bytes) > self.limit {
            if let Some((_, removed)) = self.blocks.pop() {
                self.bytes -= removed;
            } else if let Some((_, removed)) = self.texts.pop() {
                self.bytes -= removed.capacity() + std::mem::size_of::<(i64, String)>();
            } else {
                return Ok(row);
            }
        }
        self.blocks.insert(0, (rows, bytes));
        self.bytes += bytes;
        Ok(row)
    }

    pub fn text(&mut self, id: i64) -> Result<String> {
        let reusable = self.reusable()?;
        if let Some(at) = self.texts.iter().position(|(key, _)| *key == id) {
            let value = self.texts[at].1.clone();
            self.texts[..=at].rotate_right(1);
            return Ok(value);
        }
        let text: String = self.text.query_row(params![id], |r| r.get(0))?;
        let bytes = text.len() + std::mem::size_of::<(i64, String)>();
        if reusable && bytes <= self.limit / 8 {
            while self.texts.len() == 8 || self.bytes.saturating_add(bytes) > self.limit {
                if let Some((_, removed)) = self.texts.pop() {
                    self.bytes -= removed.capacity() + std::mem::size_of::<(i64, String)>();
                } else {
                    return Ok(text);
                }
            }
            self.bytes += bytes;
            self.texts.insert(0, (id, text.clone()));
        }
        Ok(text)
    }
}

fn read_block(
    c: &Connection,
    bytes: &[u8],
    count: usize,
    source: i64,
    id: i64,
    limit: usize,
) -> Result<(Relation, Option<Vec<Relation>>, usize)> {
    if !(1..=128).contains(&count) {
        return Err(super::super::packed::codec::invalid());
    }
    let mut used =
        count * std::mem::size_of::<Relation>() + std::mem::size_of::<(Vec<Relation>, usize)>();
    let mut retained = (used <= limit).then(|| Vec::with_capacity(count));
    let mut selected = None;
    decode::visit(c, bytes, count, source, |borrowed| {
        let wanted = borrowed.value(0) == ValueRef::Integer(id);
        let text = match borrowed.value(4) {
            ValueRef::Text(text) => text.len(),
            _ => 0,
        };
        used = used.saturating_add(text);
        if used > limit {
            retained = None;
        }
        // Check the byte allowance before copying any unselected external text.
        if wanted || retained.is_some() {
            let row = borrowed.into_owned(u64::MAX);
            if wanted {
                if retained.is_some() {
                    selected = Some(row.clone());
                } else {
                    selected = Some(row);
                    return Ok(());
                }
            }
            if let Some(rows) = &mut retained {
                rows.push(row);
            }
        }
        Ok(())
    })?;
    Ok((
        selected.ok_or(rusqlite::Error::QueryReturnedNoRows)?,
        retained,
        used,
    ))
}
