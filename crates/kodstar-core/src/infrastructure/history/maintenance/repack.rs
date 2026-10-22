// Purpose: 기존 내용 주소·참조를 유지하며 제한된 독립 기준으로 재압축.
// Pattern: Transactional Repacking.
// Usage: history upgrade가 검증한 저장소에서 명시적으로 실행.
// Related: crates/kodstar-core/src/infrastructure/history/content/encoding.rs, crates/kodstar-core/src/infrastructure/history/maintenance/mod.rs.

use super::verification;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::history::content::{chunk_read, encoding};
use rusqlite::{Connection, params};

#[cfg(test)]
mod tests;

pub(super) fn run(c: &Connection, cancel: &Cancellation) -> Result<()> {
    let transaction = c.unchecked_transaction()?;
    let outcome = compact(&transaction, cancel);
    crate::infrastructure::index::stop_observing(&transaction)?;
    if let Err(error) = outcome {
        transaction.finish()?;
        return Err(error);
    }
    cancel.check()?;
    transaction.commit()?;
    crate::infrastructure::index::observe_cancellation(c, cancel)
}

fn compact(c: &Connection, cancel: &Cancellation) -> Result<()> {
    // Origins and candidate windows live on disk. Repacking retains at most the
    // current 64KiB plaintext, one 64KiB base and their bounded encoded payloads.
    c.execute_batch("CREATE TEMP TABLE repack_origins(chunk INTEGER PRIMARY KEY,path BLOB);
        INSERT INTO repack_origins SELECT r.chunk,min(e.path) FROM chunk_refs r JOIN entries e ON e.blob=r.blob GROUP BY r.chunk;
        INSERT OR IGNORE INTO repack_origins SELECT r.chunk,min(w.path) FROM chunk_refs r JOIN working w ON w.blob=r.blob GROUP BY r.chunk;
        CREATE TEMP TABLE repack_pinned(id INTEGER PRIMARY KEY);
        INSERT INTO repack_pinned SELECT DISTINCT base FROM chunk_data WHERE base IS NOT NULL;
        CREATE TEMP TABLE repack_bases(id INTEGER PRIMARY KEY,path BLOB,size INTEGER);
        CREATE INDEX repack_bases_path ON repack_bases(path,id);
        CREATE INDEX repack_bases_size ON repack_bases(size,id);")?;
    let mut last = 0i64;
    loop {
        cancel.check()?;
        let mut select=c.prepare("SELECT d.id,d.codec,d.base,length(d.content),o.path,EXISTS(SELECT 1 FROM repack_pinned p WHERE p.id=d.id) FROM chunk_data d LEFT JOIN repack_origins o ON o.chunk=d.id WHERE d.id>?1 ORDER BY d.id LIMIT 1")?;
        let mut rows = select.query([last])?;
        let Some(row) = rows.next()? else {
            break;
        };
        let id: i64 = row.get(0)?;
        let mut codec: u8 = row.get(1)?;
        let mut base: Option<i64> = row.get(2)?;
        let mut best: usize = row.get(3)?;
        let path: Option<Vec<u8>> = row.get(4)?;
        let retained: bool = row.get(5)?;
        let raw = chunk_read::load(c, id)?;
        drop(rows);
        drop(select);
        let independent = encoding::encode(&raw, &[]);
        let mut payload = None;
        if independent.len() + 32 < best {
            best = independent.len();
            payload = Some(independent);
            codec = 1;
            base = None;
        }
        if !retained {
            let mut candidates=c.prepare("SELECT id FROM repack_bases WHERE path IS ?1 AND size BETWEEN ?2 AND ?3 ORDER BY id DESC LIMIT 8")?;
            let mut rows =
                candidates.query(params![path, raw.len() / 2, raw.len().saturating_mul(2)])?;
            while let Some(row) = rows.next()? {
                cancel.check()?;
                let candidate: i64 = row.get(0)?;
                let dictionary = chunk_read::load(c, candidate)?;
                let delta = encoding::encode(&raw, &dictionary);
                if delta.len() + 32 < best {
                    best = delta.len();
                    payload = Some(delta);
                    codec = 2;
                    base = Some(candidate);
                }
            }
        }
        if let Some(payload) = payload {
            if base.is_some_and(|base| base >= id) {
                return Err(Error::operation("invalid history repack base ordering"));
            }
            c.execute(
                "UPDATE chunk_data SET codec=?2,base=?3,content=?4 WHERE id=?1",
                params![id, codec, base, payload],
            )?;
        }
        if codec != 2 {
            c.execute(
                "INSERT INTO repack_bases VALUES(?1,?2,?3)",
                params![id, path, raw.len()],
            )?;
        }
        last = id;
    }
    verification::verify(c, cancel)?;
    c.execute_batch(
        "DROP TABLE repack_bases; DROP TABLE repack_origins; DROP TABLE repack_pinned;",
    )?;
    Ok(())
}
