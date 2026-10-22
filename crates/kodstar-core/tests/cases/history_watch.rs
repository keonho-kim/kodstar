// Purpose: watch 대기 중 다른 이력 명령의 실행 가능성 검증.
// Pattern: Integration Test.
// Usage: engine 감시 테스트가 기한이 있는 동시 요청을 실행.
// Related: crates/kodstar-core/src/application/execution/watch.rs.

use crate::support::*;
use kodstar_core::*;
use std::time::Duration;

#[test]
fn watch_releases_repository_lock_between_synchronizations() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn watched() {}")?;
    project.build(false)?;
    let stop = Cancellation::default();
    struct Sink<'a> {
        project: &'a Project,
        stop: Cancellation,
        read: bool,
    }
    impl QuerySink for Sink<'_> {
        fn begin(&mut self, _: &QueryMetadata) -> Result<()> {
            Ok(())
        }
        fn finish(&mut self, _: &QuerySummary) -> Result<()> {
            Ok(())
        }
        fn row(&mut self, row: &QueryRow) -> Result<()> {
            if !matches!(row, QueryRow::Init(_)) {
                return Ok(());
            }
            let project = self.project;
            let deadline = Cancellation::default();
            let worker_cancel = deadline.clone();
            let result = std::thread::scope(|scope| {
                let (send, receive) = std::sync::mpsc::channel();
                scope.spawn(move || {
                    let result = project.engine.history(
                        &project.options(),
                        &HistoryRequest::List {
                            refresh: Refresh::None,
                        },
                        &worker_cancel,
                        &mut Collected::default(),
                    );
                    let _ = send.send(result);
                });
                match receive.recv_timeout(Duration::from_secs(3)) {
                    Ok(result) => result.map(|_| ()),
                    Err(_) => {
                        deadline.cancel();
                        Err(Error::new(
                            ErrorKind::Operation,
                            "watch held the repository lock while idle",
                        ))
                    }
                }
            });
            self.read = result.is_ok();
            self.stop.cancel();
            result
        }
    }
    let mut sink = Sink {
        project: &project,
        stop: stop.clone(),
        read: false,
    };
    let error = project
        .engine
        .watch(&project.options(), &stop, &mut sink)
        .expect_err("watch cancelled after independent history read");
    assert!(sink.read);
    assert_eq!(error.kind, ErrorKind::Cancelled);
    Ok(())
}
