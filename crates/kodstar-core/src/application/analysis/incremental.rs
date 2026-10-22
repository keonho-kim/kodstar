// Purpose: 사실 재사용과 변경 파일 해석을 허용하는 보수적인 조건 판정.
// Pattern: Policy.
// Usage: writer가 환경이 같은 본문 후보에만 검사하고 설정·삭제는 분석이 반영.
// Related: crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/src/domain/facts/fingerprint.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::index::FileRecord;
use crate::infrastructure::index::invalidation;
use rusqlite::Connection;

#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Resolution {
    #[default]
    None,
    ChangedFiles,
    All,
}

pub(super) fn reusable_bodies(connection: &Connection, record: &FileRecord) -> Result<bool> {
    // Identical extraction environments keep Go declarations, imports and method sets stable.
    // The partial resolver separately rebuilds dispatch for the replaced call facts.
    if record.language == "go" {
        return Ok(true);
    }
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM files WHERE language='rust' AND status='analyzed')
         AND NOT EXISTS(SELECT 1 FROM files WHERE status='analyzed' AND language<>'rust')
         AND NOT EXISTS(SELECT 1 FROM symbols WHERE kind<>'file'
             AND (language<>'rust' OR kind NOT IN ('function','module')))
         AND NOT EXISTS(SELECT 1 FROM bindings b JOIN files f ON f.id=b.file_id
             WHERE f.language='rust' AND b.kind NOT IN ('function','parameter','import','local','module'))
         AND NOT EXISTS(SELECT 1 FROM facts WHERE relation IN
             ('inherits','implements','mixes_in','overrides','dispatch_candidate'))",
        [], |r| r.get(0),
    )?)
}

pub(super) fn scoped_declarations(connection: &Connection, record: &FileRecord) -> Result<bool> {
    if record.language != "go" || !invalidation::only_go(connection)? {
        return Ok(false);
    }
    if let Some(id) = invalidation::prior_file(connection, record)?
        && !invalidation::package_functions(connection, id)?
    {
        return Ok(false);
    }
    invalidation::capture(connection, record)?;
    Ok(true)
}
