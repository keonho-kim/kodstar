// Purpose: 정렬된 key staging을 한 블록씩 합치며 안정 ID를 매핑.
// Pattern: Streaming Merge.
// Usage: dependencies가 생성된 dictionary_pending을 소비하고 제거.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/mod.rs, crates/kodstar-core/src/infrastructure/index/dependencies/mod.rs.

use super::super::packed::codec::invalid;
use rusqlite::{Connection, OptionalExtension, Result, params};

struct Pending {
    block: i64,
    kind: String,
    keys: Vec<(i64, String)>,
    next: Option<String>,
    dirty: bool,
}

impl Pending {
    fn fits(&self, kind: &str, key: &str) -> bool {
        self.kind == kind
            && self
                .keys
                .first()
                .is_none_or(|(_, first)| first.as_str() <= key)
            && self.next.as_ref().is_none_or(|next| key < next.as_str())
            && (self.keys.len() < 32
                || self
                    .keys
                    .last()
                    .is_some_and(|(_, last)| key <= last.as_str()))
    }

    fn load(c: &Connection, kind: String, key: &str) -> Result<Self> {
        let old:Option<(i64,String,usize,Vec<u8>)>=c.query_row("SELECT id,last,count,payload FROM dictionary_blocks WHERE kind=?1 AND first<=?2 ORDER BY first DESC LIMIT 1",params![kind,key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (block, keys) = match old {
            Some((id, last, count, bytes)) if count < 32 || last.as_str() >= key => {
                (id, super::codec::decode(&bytes, count)?)
            }
            _ => (0, Vec::new()),
        };
        let next=c.query_row("SELECT first FROM dictionary_blocks WHERE kind=?1 AND first>?2 ORDER BY first LIMIT 1",params![kind,key],|r|r.get(0)).optional()?;
        Ok(Self {
            block,
            kind,
            keys,
            next,
            dirty: false,
        })
    }

    fn flush(self, c: &Connection) -> Result<()> {
        if self.dirty {
            super::store(c, self.block, &self.kind, &self.keys)?;
        }
        Ok(())
    }

    fn split(&mut self, c: &Connection) -> Result<()> {
        if self.keys.len() <= 32
            && (self.keys.len() <= 1
                || self.keys.iter().map(|(_, k)| k.len()).sum::<usize>() <= 65536)
        {
            return Ok(());
        }
        let (mut part, mut bytes) = (Vec::new(), 0);
        for key in std::mem::take(&mut self.keys) {
            if !part.is_empty() && (part.len() == 32 || bytes + key.1.len() > 65536) {
                super::store(c, self.block, &self.kind, &part)?;
                self.block = 0;
                part.clear();
                bytes = 0;
            }
            bytes += key.1.len();
            part.push(key);
        }
        self.block = super::store(c, self.block, &self.kind, &part)?;
        self.keys = part;
        self.dirty = false;
        Ok(())
    }
}

pub(crate) fn publish(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE dictionary_pending(kind TEXT,lookup_key TEXT,key_id INTEGER,PRIMARY KEY(kind,lookup_key)) WITHOUT ROWID;
")?;
    let mut select = c.prepare(
        "SELECT DISTINCT kind,lookup_key FROM resolution_dependencies ORDER BY kind,lookup_key",
    )?;
    let mut input = select.query([])?;
    let mut mapped = c.prepare("INSERT INTO dictionary_pending VALUES(?1,?2,?3)")?;
    let mut next: i64 = c.query_row("SELECT coalesce(max(id),0) FROM dictionary_ids", [], |r| {
        r.get(0)
    })?;
    let mut current: Option<Pending> = None;
    while let Some(row) = input.next()? {
        let kind: String = row.get(0)?;
        let key: String = row.get(1)?;
        if key.len() > 65536 {
            if let Some(old) = current.take() {
                old.flush(c)?;
            }
            let id = super::large::intern(c, &kind, &key, &mut next)?;
            mapped.execute(params![kind, key, id])?;
            continue;
        }
        if !current
            .as_ref()
            .is_some_and(|pending| pending.fits(&kind, &key))
        {
            if let Some(old) = current.take() {
                old.flush(c)?;
            }
            current = Some(Pending::load(c, kind.clone(), &key)?);
        }
        let pending = current.as_mut().ok_or_else(invalid)?;
        let id = match pending.keys.binary_search_by(|(_, v)| v.cmp(&key)) {
            Ok(at) => pending.keys[at].0,
            Err(at) => {
                next = next.checked_add(1).ok_or_else(invalid)?;
                pending.keys.insert(at, (next, key.clone()));
                pending.dirty = true;
                next
            }
        };
        mapped.execute(params![kind, key, id])?;
        pending.split(c)?;
    }
    if let Some(pending) = current {
        pending.flush(c)?;
    }
    Ok(())
}
