// Purpose: native 완료 상태를 현재 파일과 사람이 읽는 진행 진단으로 변환.
// Pattern: Progress Projection.
// Usage: 파일 writer가 관측을 저장할 때 native 미완료·제한을 알림.
// Related: crates/kodstar-core/src/application/analysis/pipeline/writer.rs, crates/kodstar-core/src/domain/native.rs.

use crate::domain::{diagnostics::Result, native::Report, results::Progress};
use std::path::Path;

pub(super) fn native(
    report: Option<&Report>,
    path: &Path,
    emit: &mut dyn FnMut(&Progress) -> Result<()>,
) -> Result<()> {
    let Some(report) = report.filter(|r| r.status != "complete") else {
        return Ok(());
    };
    let phase = if report.status == "limited" {
        "native_limited"
    } else {
        "native_incomplete"
    };
    let detail = report
        .detail
        .as_deref()
        .and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok());
    let message = detail
        .as_ref()
        .map(|d| {
            let kind = d["kind"].as_str().unwrap_or("native context");
            let subject = d["subject"].as_str().unwrap_or("");
            if d["limit"].is_u64()
                && !matches!(
                    kind,
                    "MissingHeader" | "CompilerError" | "CompileCommand" | "Provider"
                )
            {
                format!("{kind}: used {}, limit {}", d["used"], d["limit"])
            } else {
                format!("{kind}: {subject}")
            }
        })
        .unwrap_or_else(|| "native semantic proof unavailable".into());
    emit(&Progress::task(
        phase,
        path.to_string_lossy().into_owned(),
        Some(message),
    ))
}
