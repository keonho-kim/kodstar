// Purpose: native 큐의 단일 소유·일반 입력 병행과 취소 시 자원 반환 검증.
// Pattern: Concurrency Contract Test.
// Usage: routing unit tests가 결정적인 bounded channel 상태를 구성.
// Related: crates/kodstar-core/src/application/analysis/routing.rs, crates/kodstar-core/src/application/analysis/budget.rs.

use super::*;
use crate::{
    application::analysis::{budget::ByteBudget, events::InputKind},
    domain::languages::Language,
    infrastructure::index::FileRecord,
};
use std::sync::Arc;

fn work(kind: InputKind, bytes: &Arc<ByteBudget>) -> Result<Work> {
    Ok(Work {
        kind,
        record: FileRecord {
            path: "test.rs".into(),
            language: "rust".into(),
            module: String::new(),
            package: String::new(),
            content_hash: [0; 32],
            size: 0,
            status: "analyzed",
        },
        source: Vec::new(),
        previous: None,
        prefix: None,
        _input: bytes.reserve(1, &Cancellation::default())?,
    })
}

#[test]
fn only_the_native_owner_receives_c_and_cpp_inputs() -> Result<()> {
    let bytes = ByteBudget::new(2)?;
    let (general, general_rx) = channel::bounded(1);
    let (native, native_rx) = channel::bounded(1);
    let router = Router { general, native };
    router.send(
        work(InputKind::Code(Language::C), &bytes)?,
        &Cancellation::default(),
    )?;
    assert!(general_rx.try_recv().is_err());
    assert!(
        native_rx
            .try_recv()
            .map_err(|e| Error::caused_by("receive native work", e))?
            .kind
            .is_native()
    );
    router.send(
        work(InputKind::Code(Language::Rust), &bytes)?,
        &Cancellation::default(),
    )?;
    assert!(native_rx.try_recv().is_err());
    assert!(
        !general_rx
            .try_recv()
            .map_err(|e| Error::caused_by("receive general work", e))?
            .kind
            .is_native()
    );
    Ok(())
}

#[test]
fn native_owner_prioritizes_its_queue_then_drains_general_work_and_closes() -> Result<()> {
    let bytes = ByteBudget::new(2)?;
    let (general, general_rx) = channel::bounded(1);
    let (native, native_rx) = channel::bounded(1);
    let router = Router { general, native };
    router.send(
        work(InputKind::Code(Language::Rust), &bytes)?,
        &Cancellation::default(),
    )?;
    router.send(
        work(InputKind::Code(Language::Cpp), &bytes)?,
        &Cancellation::default(),
    )?;
    drop(router);
    let mut inbox = Inbox::new(general_rx, Some(native_rx));
    assert!(inbox.native_owner());
    assert!(
        inbox
            .receive()
            .map_err(|e| Error::caused_by("native queue", e))?
            .kind
            .is_native()
    );
    assert!(
        !inbox
            .receive()
            .map_err(|e| Error::caused_by("general queue", e))?
            .kind
            .is_native()
    );
    assert!(matches!(
        inbox.receive(),
        Err(channel::RecvTimeoutError::Disconnected)
    ));
    let _all = bytes.reserve(2, &Cancellation::default())?;
    Ok(())
}

#[test]
fn full_native_queue_cancellation_joins_producer_and_releases_input_permits() -> Result<()> {
    let bytes = ByteBudget::new(2)?;
    let (general, _) = channel::bounded(1);
    let (native, native_rx) = channel::bounded(1);
    let router = Router { general, native };
    router.send(
        work(InputKind::Code(Language::Cpp), &bytes)?,
        &Cancellation::default(),
    )?;
    let next = work(InputKind::Code(Language::C), &bytes)?;
    let cancel = Cancellation::default();
    let worker_cancel = cancel.clone();
    let (started, ready) = channel::bounded(1);
    let (done, finished) = channel::bounded(1);
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        done.send(router.send(next, &worker_cancel)).unwrap();
    });
    ready
        .recv()
        .map_err(|e| Error::caused_by("producer ready", e))?;
    cancel.cancel();
    let outcome = finished.recv_timeout(Duration::from_secs(2));
    drop(native_rx);
    worker
        .join()
        .map_err(|_| Error::operation("input producer panicked"))?;
    assert!(
        matches!(outcome, Ok(Err(error)) if error.kind == crate::domain::diagnostics::ErrorKind::Cancelled)
    );
    let _all = bytes.reserve(2, &Cancellation::default())?;
    Ok(())
}
