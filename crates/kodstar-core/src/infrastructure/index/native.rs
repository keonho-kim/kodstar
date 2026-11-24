// Purpose: native 입력 관측과 완료 진단을 의미 사실과 분리해 원자적으로 교체.
// Pattern: Storage Adapter.
// Usage: pipeline의 파일 완료·사실 재사용 경로가 같은 transaction에서 호출.
// Related: crates/kodstar-core/src/domain/native.rs, crates/kodstar-core/src/infrastructure/index/schema.sql.

use crate::domain::{
    diagnostics::Result,
    native::{Observation, Report},
};
use rusqlite::{Connection, params};

pub(crate) fn replace(c: &Connection, file: i64, report: Option<&Report>) -> Result<()> {
    c.execute("DELETE FROM native_analysis WHERE file_id=?1", [file])?;
    c.execute("DELETE FROM diagnostics WHERE file_id=?1 AND code IN ('clang_semantic_limit','clang_semantic_incomplete')",[file])?;
    let Some(report) = report else { return Ok(()) };
    c.execute(
        "INSERT INTO native_analysis VALUES(?1,?2,?3,?4)",
        params![
            file,
            super::hash_bytes(&report.environment)?.as_slice(),
            report.status,
            report.detail
        ],
    )?;
    let mut insert = c.prepare("INSERT INTO native_inputs VALUES(?1,?2,?3,?4)")?;
    for (path, value) in &report.inputs {
        let (kind, hash) = match value {
            Observation::Missing => (0, None),
            Observation::Directory => (1, None),
            Observation::File(hash) => (2, Some(*hash)),
        };
        insert.execute(params![
            file,
            path,
            kind,
            hash.as_ref().map(|h| h.as_slice())
        ])?;
    }
    if report.status != "complete" {
        let code = if report.status == "limited" {
            "clang_semantic_limit"
        } else {
            "clang_semantic_incomplete"
        };
        c.execute(
            "INSERT INTO diagnostics(file_id,code,message) VALUES(?1,?2,?3)",
            params![
                file,
                code,
                report
                    .detail
                    .as_deref()
                    .unwrap_or("Clang semantic proof is unavailable for this compilation context")
            ],
        )?;
    }
    Ok(())
}
