// Purpose: 큰 후보 정보의 내용 주소 공유와 블록 참조 수명 검증.
// Pattern: Content Addressed Storage.
// Usage: 관계 writer가 같은 transaction에서 후보 그룹을 등록.
// Related: crates/kodstar-core/src/infrastructure/index/relations/sets/schema.sql, crates/kodstar-core/src/infrastructure/index/relations/groups.rs.

pub(super) mod targets;
#[cfg(test)]
mod tests;
use crate::infrastructure::index::packed::codec::{self, Input};
use rusqlite::{Connection, Result, Statement, params};

pub(super) struct Writer<'a> {
    insert: Statement<'a>,
    read: Statement<'a>,
}

impl<'a> Writer<'a> {
    pub fn new(c: &'a Connection) -> Result<Self> {
        Ok(Self {
            insert: c.prepare("INSERT INTO relation_sets(digest,count,payload,refs) VALUES(?1,?2,?3,0) ON CONFLICT(digest) DO NOTHING")?,
            read: c.prepare("SELECT id,count,payload FROM relation_sets WHERE digest=?1")?,
        })
    }

    pub fn intern(&mut self, bytes: &[u8], count: usize) -> Result<i64> {
        if bytes.len() < 64 {
            return Ok(0);
        }
        let digest = blake3::hash(bytes);
        self.insert
            .execute(params![digest.as_bytes(), count, bytes])?;
        self.read.query_row([digest.as_bytes()], |r| {
            if r.get::<_, usize>(1)? != count || r.get_ref(2)?.as_blob()? != bytes {
                return Err(codec::invalid());
            }
            r.get(0)
        })
    }
}

pub(super) fn register(c: &Connection) -> Result<()> {
    super::super::packed::expansion::register(c, c"kodstar_relation_set_rows", |bytes, count| {
        let count = usize::try_from(count).map_err(|_| codec::invalid())?;
        Ok(super::groups::references(bytes, count)?
            .into_iter()
            .map(|id| [id, 0])
            .collect())
    })
}

pub(super) fn verify(c: &Connection) -> Result<()> {
    let mut q = c.prepare("SELECT count,payload,digest,refs FROM relation_sets")?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        let count: usize = row.get(0)?;
        let payload = row.get_ref(1)?.as_blob()?;
        if !(1..=128).contains(&count)
            || row.get_ref(2)?.as_blob()? != blake3::hash(payload).as_bytes()
            || row.get::<_, i64>(3)? <= 0
        {
            return Err(codec::invalid());
        }
        let mut input = Input(payload);
        for _ in 0..count {
            targets::read(&mut input)?;
        }
        input.finish()?;
    }
    Ok(())
}
