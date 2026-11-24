// Purpose: 연속 관계 ID 구간의 정확 검색과 JSONB 배치의 구간 집계.
// Pattern: Interval Index.
// Usage: 관계 cursor·삭제·writer가 같은 물리 주소 계약을 사용.
// Related: crates/kodstar-core/src/infrastructure/index/relations/schema.sql, crates/kodstar-core/src/infrastructure/index/relations/writer.rs.

use rusqlite::{Connection, OptionalExtension, Result, params, types::Value};

pub(super) const RANGES: &str = "SELECT min(id) AS first,max(id) AS last FROM (
    SELECT json_extract(value,'$[0]') AS id,
      json_extract(value,'$[0]')-row_number() OVER(ORDER BY json_extract(value,'$[0]')) AS sequence
    FROM jsonb_each(?1)) GROUP BY sequence";

pub(super) fn find(c: &Connection, id: &Value, after: i64) -> Result<Option<i64>> {
    super::super::packed::statements::row(c,"SELECT block FROM relation_id_ranges
        WHERE first=(SELECT first FROM relation_id_ranges WHERE first<=?1 ORDER BY first DESC LIMIT 1)
          AND last>=?1 AND block>?2", params![id,after], |r| r.get(0)).optional()
}
