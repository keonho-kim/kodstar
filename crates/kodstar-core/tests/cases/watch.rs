// Purpose: 실제 변경 감시·세대 공개와 취소 후 자원 정리 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/src/application/execution/watch.rs.

use crate::support::*;
use kodstar_core::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn watching_publishes_saved_changes_and_finishes_on_cancellation() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn before() {}")?;
    let first = project.build(false)?;
    let cancel = Cancellation::default();
    struct Sink<'a> {
        project: &'a Project,
        cancel: Cancellation,
        rows: u64,
        last: i64,
        summary: Option<QuerySummary>,
    }
    impl QuerySink for Sink<'_> {
        fn begin(&mut self, _: &QueryMetadata) -> Result<()> {
            Ok(())
        }
        fn row(&mut self, row: &QueryRow) -> Result<()> {
            if let QueryRow::Init(report) = row {
                self.rows += 1;
                self.last = report.generation;
                if self.rows == 1 {
                    self.project
                        .write("main.rs", "fn after() {}")
                        .map_err(|error| Error::caused_by("edit watched fixture", error))?;
                } else {
                    self.cancel.cancel();
                }
            }
            Ok(())
        }
        fn finish(&mut self, summary: &QuerySummary) -> Result<()> {
            self.summary = Some(summary.clone());
            Ok(())
        }
    }
    let mut sink = Sink {
        project: &project,
        cancel: cancel.clone(),
        rows: 0,
        last: 0,
        summary: None,
    };
    std::thread::scope(|scope| {
        let (done, timeout) = mpsc::channel();
        let deadline = cancel.clone();
        scope.spawn(move || {
            if timeout.recv_timeout(Duration::from_secs(10)).is_err() {
                deadline.cancel();
            }
        });
        let result = project.engine.watch(&project.options(), &cancel, &mut sink);
        let _ = done.send(());
        assert_eq!(
            result.expect_err("watch ends on cancellation").kind,
            ErrorKind::Cancelled
        );
    });
    assert_eq!(
        sink.rows, 2,
        "native watcher did not observe the edit before the deadline"
    );
    assert_eq!(sink.last, first.generation + 1);
    assert!(!sink.summary.ok_or("missing watch completion")?.complete);
    let mut cleaned = Collected::default();
    project.engine.cache(
        &project.options(),
        CacheRequest::Clean { all: true },
        &Cancellation::default(),
        &mut cleaned,
    )?;
    assert!(
        cleaned
            .rows
            .iter()
            .any(|row| matches!(row, QueryRow::Cache(cache) if cache.action == "removed"))
    );
    Ok(())
}
