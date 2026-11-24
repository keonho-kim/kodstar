// Purpose: 추출한 호출의 실제 정수 필드를 대조하고 이전 fact를 한 번만 소비.
// Pattern: Exact Reuse.
// Usage: FactWriter가 본문 journal을 사용 중인 파일에 대해 지연 생성.
// Related: crates/kodstar-core/src/infrastructure/index/body_reuse/mod.rs, crates/kodstar-core/src/infrastructure/index/facts.rs.

use crate::domain::diagnostics::{Error, Result};
use rusqlite::{Connection, OptionalExtension, Statement, StatementStatus, params};

pub(crate) struct Matcher<'a> {
    find: Statement<'a>,
    consume: Statement<'a>,
}

impl<'a> Matcher<'a> {
    pub fn new(c: &'a Connection, budget: usize) -> Result<Self> {
        let matcher = Self {
            find: c.prepare(&format!(
                "SELECT b.fact_id,{},b.native_module FROM body_original b
                CROSS JOIN facts_rows f ON f.id=b.fact_id
                WHERE b.file_id=?1 AND b.digest=?2 ORDER BY b.fact_id LIMIT 1",
                super::FIELDS
            ))?,
            consume: c.prepare(
                "DELETE FROM body_original WHERE file_id=?1 AND digest=?2 AND fact_id=?3",
            )?,
        };
        let bytes = matcher.find.get_status(StatementStatus::MemUsed) as usize
            + matcher.consume.get_status(StatementStatus::MemUsed) as usize;
        if bytes > budget {
            return Err(Error::new(
                crate::domain::diagnostics::ErrorKind::Limit,
                "body reuse statements exceed the working-memory read budget",
            ));
        }
        Ok(matcher)
    }

    pub fn matches(&mut self, values: &[i64; 15]) -> Result<bool> {
        let digest = super::digest(values);
        let found = self
            .find
            .query_row(params![values[0], digest.as_bytes()], |r| {
                Ok((r.get::<_, i64>(0)?, super::fields(r, 1)?))
            })
            .optional();
        self.find.clear_bindings();
        let Some((id, previous)) = found? else {
            return Ok(false);
        };
        if &previous != values {
            return Err(Error::operation("body fact digest mismatch"));
        }
        let consumed = self
            .consume
            .execute(params![values[0], digest.as_bytes(), id]);
        self.consume.clear_bindings();
        consumed?;
        Ok(true)
    }
}
