// Purpose: Reuse one verified base only when its complete stored representation is unchanged.
// Pattern: Bounded Content Cache.
// Usage: chunk_read workspace lends the base to the current dependent-frame decode.
// Related: crates/kodstar-core/src/infrastructure/history/content/chunk_read.rs.

use super::{decode, encoding};
use crate::domain::diagnostics::Result;
use rusqlite::{Row, types::ValueRef};
use std::borrow::Cow;

#[derive(Default)]
pub(super) struct Base {
    value: Option<([u8; 32], Vec<u8>)>,
}

impl Base {
    pub fn read<'a>(
        &'a mut self,
        row: &Row<'_>,
        decoder: &mut encoding::Decoder,
        limit: usize,
    ) -> Result<Cow<'a, [u8]>> {
        let limit = limit.min(encoding::MAX);
        if self
            .value
            .as_ref()
            .is_some_and(|(_, bytes)| bytes.capacity() > limit)
        {
            self.value = None;
        }
        if limit == 0 {
            return Ok(Cow::Owned(decode(row, &[], decoder)?));
        }
        let mut hash = blake3::Hasher::new();
        // Hash the current payload AND every checked field. An ID or the declared
        // raw-content hash alone would hide corruption, writes or rollback changes.
        for column in 0..8 {
            match row.get_ref(column)? {
                ValueRef::Null => {
                    hash.update(&[0]);
                }
                ValueRef::Integer(value) => {
                    hash.update(&[1]);
                    hash.update(&value.to_le_bytes());
                }
                ValueRef::Blob(bytes) => {
                    hash.update(&[2]);
                    hash.update(&(bytes.len() as u64).to_le_bytes());
                    hash.update(bytes);
                }
                _ => return Err(super::invalid()),
            }
        }
        let key = *hash.finalize().as_bytes();
        if self
            .value
            .as_ref()
            .is_none_or(|(previous, _)| *previous != key)
        {
            // Release the previous source before decoding its replacement.
            self.value = None;
            let bytes = decode(row, &[], decoder)?;
            if bytes.capacity() > limit {
                return Ok(Cow::Owned(bytes));
            }
            self.value = Some((key, bytes));
        }
        Ok(Cow::Borrowed(
            &self.value.as_ref().ok_or_else(super::invalid)?.1,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, params};

    fn load(
        c: &Connection,
        base: &mut Base,
        decoder: &mut encoding::Decoder,
        limit: usize,
    ) -> Result<Vec<u8>> {
        let mut q = c.prepare("SELECT * FROM base")?;
        let mut rows = q.query([])?;
        Ok(base
            .read(rows.next()?.unwrap(), decoder, limit)?
            .into_owned())
    }

    #[test]
    fn cached_bases_never_hide_payload_or_metadata_corruption_and_rollback() -> Result<()> {
        let c = Connection::open_in_memory()?;
        c.execute_batch("CREATE TABLE base(hash BLOB,size INTEGER,codec INTEGER,parent INTEGER,lines INTEGER,first INTEGER,last INTEGER,content BLOB)")?;
        let raw = b"one\ntwo\n".repeat(1024);
        let encoded = encoding::Encoder::default().encode(&raw, &[])?;
        let (lines, first, last) = super::super::super::chunks::line_info(&raw);
        c.execute(
            "INSERT INTO base VALUES(?1,?2,1,NULL,?3,?4,?5,?6)",
            params![
                blake3::hash(&raw).as_bytes(),
                raw.len(),
                lines,
                first,
                last,
                encoded
            ],
        )?;
        let mut base = Base::default();
        let mut decoder = encoding::Decoder::default();
        assert_eq!(load(&c, &mut base, &mut decoder, encoding::MAX)?, raw);
        assert!(base.value.is_some());
        assert_eq!(load(&c, &mut base, &mut decoder, encoding::MAX)?, raw);
        for update in ["UPDATE base SET content=X'00'", "UPDATE base SET lines=0"] {
            c.execute_batch("SAVEPOINT corrupt")?;
            c.execute_batch(update)?;
            assert!(load(&c, &mut base, &mut decoder, encoding::MAX).is_err());
            c.execute_batch("ROLLBACK TO corrupt;RELEASE corrupt")?;
            assert_eq!(load(&c, &mut base, &mut decoder, encoding::MAX)?, raw);
        }
        assert_eq!(load(&c, &mut base, &mut decoder, 1)?, raw);
        assert!(base.value.is_none());
        Ok(())
    }
}
