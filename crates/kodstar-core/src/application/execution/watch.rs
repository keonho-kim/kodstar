// Purpose: 변경 신호의 debounce·재스캔·갱신과 watch 취소 정책.
// Pattern: Coordinator.
// Usage: Engine watch API가 프로젝트·취소·결과 sink를 전달.
// Related: crates/kodstar-core/src/infrastructure/watch.rs, crates/kodstar-core/src/application/execution/context.rs.

use super::context::Context;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Diagnostic, ErrorKind, Result},
    results::{QueryRow, QuerySink, QuerySummary},
};
use std::time::{Duration, Instant};

pub(crate) fn run(
    context: &mut Context<'_>,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<QuerySummary> {
    let project = context.required()?;
    let initial = project.report.take().ok_or_else(|| {
        crate::domain::diagnostics::Error::operation("watch initial synchronization is missing")
    })?;
    let changes = project
        .changes
        .take()
        .ok_or_else(|| crate::domain::diagnostics::Error::operation("watch observer is missing"))?;
    sink.begin(&initial.metadata(crate::domain::query::Refresh::Metadata))?;
    let mut summary = QuerySummary {
        complete: true,
        result_count: 1,
        unresolved_count: initial.coverage.unresolved,
        ..QuerySummary::default()
    };
    sink.row(&QueryRow::Init(initial))?;
    let period = Duration::from_secs(if changes.polling_only() { 1 } else { 30 });
    let mut last_scan = Instant::now();
    let mut last_event = None;
    let outcome: Result<()> = (|| {
        loop {
            cancel.check()?;
            if changes.take_changes() {
                last_event = Some(Instant::now());
            }
            if let Some(message) = changes.take_error()? {
                sink.row(&QueryRow::Diagnostic(Diagnostic {
                    code: "watch_rescan".into(),
                    message,
                    file: None,
                    line: None,
                }))?;
                summary.result_count += 1;
            }
            if last_scan.elapsed() >= period
                || last_event
                    .is_some_and(|instant: Instant| instant.elapsed() >= Duration::from_millis(200))
            {
                let outcome = context
                    .synchronize(cancel, &mut |event| sink.progress(event))
                    .and_then(|()| {
                        context.required()?.report.take().ok_or_else(|| {
                            crate::domain::diagnostics::Error::operation(
                                "watch synchronization report is missing",
                            )
                        })
                    });
                match outcome {
                    Ok(report) => {
                        summary.unresolved_count = report.coverage.unresolved;
                        if report.changed {
                            sink.row(&QueryRow::Init(report))?;
                            summary.result_count += 1;
                        }
                    }
                    Err(error) if error.kind == ErrorKind::Cancelled => return Err(error),
                    Err(error) => {
                        sink.row(&QueryRow::Diagnostic(Diagnostic {
                            code: "watch_update_failed".into(),
                            message: error.to_string(),
                            file: None,
                            line: None,
                        }))?;
                        summary.result_count += 1;
                    }
                }
                last_event = None;
                last_scan = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    summary.complete = false;
    summary.reasons.push(
        if outcome
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Cancelled)
        {
            "cancelled"
        } else {
            "watch_failed"
        },
    );
    sink.finish(&summary)?;
    outcome?;
    Ok(summary)
}
