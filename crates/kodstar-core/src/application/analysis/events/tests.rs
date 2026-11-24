// Purpose: 이벤트 전송의 취소·writer 종료 시 대기 해제와 permit 반환 검증.
// Pattern: Concurrency Contract Test.
// Usage: events 모듈의 작은 channel·예산 fixture로 실행.
// Related: crates/kodstar-core/src/application/analysis/events.rs, crates/kodstar-core/src/application/analysis/budget.rs.

use super::*;
use std::thread;

#[test]
fn cancellation_releases_a_full_queue_without_waiting_for_the_receiver() -> Result<()> {
    let (sender, receiver) = channel::bounded(1);
    let bytes = ByteBudget::new(4096)?;
    let cancel = Cancellation::default();
    let events = Events {
        sender,
        bytes: Arc::clone(&bytes),
        cancel: cancel.clone(),
        failure: Arc::default(),
    };
    let event = || Event::End {
        partial: None,
        fragments: Vec::new(),
        native: None,
        path: Arc::default(),
        fingerprints: Fingerprints {
            all: [0; 32],
            environment: [0; 32],
        },
    };
    events.send(event())?;
    let (done, finished) = channel::bounded(1);
    let worker = thread::spawn(move || {
        let _ = done.send(events.send(event()));
    });
    cancel.cancel();
    let result = finished.recv_timeout(Duration::from_secs(2));
    drop(receiver);
    worker
        .join()
        .map_err(|_| Error::operation("test producer panicked"))?;
    let error = result
        .map_err(|e| Error::caused_by("cancelled producer did not stop", e))?
        .expect_err("cancelled send succeeded");
    assert!(matches!(
        error.kind,
        crate::domain::diagnostics::ErrorKind::Cancelled
    ));
    let _all = bytes.reserve(4096, &Cancellation::default())?;
    Ok(())
}

#[test]
fn closing_the_writer_releases_queued_and_waiting_producers() -> Result<()> {
    let (sender, receiver) = channel::bounded(1);
    let cancel = Cancellation::default();
    let bytes = ByteBudget::new(4096)?;
    let events = Events {
        sender,
        bytes: Arc::clone(&bytes),
        cancel: cancel.clone(),
        failure: Arc::default(),
    };
    let inbox = Inbox {
        receiver,
        cancel: cancel.clone(),
    };
    let (started, ready) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        events.send(Event::End {
            partial: None,
            fragments: Vec::new(),
            native: None,
            path: Arc::default(),
            fingerprints: Fingerprints {
                all: [0; 32],
                environment: [0; 32],
            },
        })?;
        started
            .send(())
            .map_err(|error| Error::caused_by("test producer ready", error))?;
        events.send(Event::End {
            partial: None,
            fragments: Vec::new(),
            native: None,
            path: Arc::default(),
            fingerprints: Fingerprints {
                all: [0; 32],
                environment: [0; 32],
            },
        })
    });
    ready
        .recv()
        .map_err(|error| Error::caused_by("wait for test producer", error))?;
    drop(inbox);
    let result = worker
        .join()
        .map_err(|_| Error::operation("test producer panicked"))?;
    assert!(result.is_err());
    assert!(cancel.is_cancelled());
    let _all = bytes.reserve(4096, &Cancellation::default())?;
    Ok(())
}
