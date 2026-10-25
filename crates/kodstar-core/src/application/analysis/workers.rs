// Purpose: parser worker 수명·실패·취소와 파일별 배치 추출 실행.
// Pattern: Worker.
// Usage: pipeline이 scoped thread로 실행하고 종료를 기다림.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/application/analysis/batches.rs.

use super::events::{Event, Events, Work};
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Fact, Fingerprinter};
use crate::domain::source::SourceSpan;
use crate::infrastructure::parse::ParseSession;
use crossbeam_channel as channel;
use std::sync::Arc;
use std::time::Duration;

pub(super) fn run(receiver: channel::Receiver<Work>, events: Events, fact_limit: usize) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut parser = None;
        loop {
            if events.cancel.is_cancelled() {
                break;
            }
            let work = receiver.recv_timeout(Duration::from_millis(25));
            let work = match work {
                Ok(work) => work,
                Err(channel::RecvTimeoutError::Timeout) => continue,
                Err(channel::RecvTimeoutError::Disconnected) => break,
            };
            let parser = parser.get_or_insert_with(|| ParseSession::new(fact_limit));
            parser.begin_file(work.previous.is_some());
            let result = extract(parser, work, &events, fact_limit);
            parser.end_file();
            if let Err(error) = result {
                events.fail(error);
                break;
            }
        }
    }));
    if outcome.is_err() {
        events.fail(Error::operation("analysis worker panicked"));
    }
}

fn extract(
    parser: &mut ParseSession,
    work: Work,
    events: &Events,
    fact_limit: usize,
) -> Result<()> {
    let path = Arc::new(work.record.path.clone());
    let mut preserve = false;
    let ready = if let Some(previous) = &work.previous {
        let mut fingerprint = Fingerprinter::default();
        let mut replay = super::replay::Replay::new(fact_limit);
        super::extraction::run(
            parser,
            &path,
            &work.source,
            work.kind,
            &events.cancel,
            &mut |language, mut fact: Fact| {
                fingerprint.push(language, &mut fact);
                replay.push(language, fact);
                Ok(())
            },
        )?;
        let next = fingerprint.finish();
        if next.all == previous.all {
            return events.send(Event::UnchangedFacts {
                record: work.record,
                span: SourceSpan::at(&work.source, 0, work.source.len()),
            });
        }
        preserve = next.environment == previous.environment;
        Some((next, replay.finish()))
    } else {
        None
    };
    if !matches!(ready, Some((_, Some(_)))) {
        parser.replay();
    }
    events.send(Event::Begin {
        record: work.record,
        preserve,
    })?;
    if let Some((fingerprints, Some(facts))) = ready {
        if !facts.is_empty() {
            events.send(Event::Facts {
                path: Arc::clone(&path),
                facts,
            })?;
        }
        return events.send(Event::End { path, fingerprints });
    }
    let mut fingerprint = Fingerprinter::default();
    let mut batch = super::batches::BatchSender::new(events, Arc::clone(&path), fact_limit);
    super::extraction::run(
        parser,
        &path,
        &work.source,
        work.kind,
        &events.cancel,
        &mut |language, mut fact| {
            fingerprint.push(language, &mut fact);
            batch.push(language, fact)
        },
    )?;
    batch.finish()?;
    events.send(Event::End {
        path,
        fingerprints: fingerprint.finish(),
    })
}
