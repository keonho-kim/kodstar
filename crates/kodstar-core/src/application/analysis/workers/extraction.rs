// Purpose: 파일 사실의 부수 효과 없는 사전 추출과 검증된 결과 재생.
// Pattern: Functional Core / Imperative Shell.
// Usage: workers가 파일별 실패를 처리하기 전에 호출.
// Related: crates/kodstar-core/src/application/analysis/workers.rs, crates/kodstar-core/src/application/analysis/replay.rs.

use super::super::events::{Event, Events, FileChange, Work};
use crate::domain::{
    diagnostics::{Error, ErrorKind, Result},
    facts::{Fact, Fingerprinter},
    source::SourceSpan,
};
use crate::infrastructure::parse::ParseSession;
use std::sync::Arc;

mod append;

pub(super) fn run(
    parser: &mut ParseSession,
    work: &Work,
    events: &Events,
    fact_limit: usize,
) -> Result<()> {
    let path = Arc::new(work.record.path.clone());
    let mut append =
        work.prefix
            .as_ref()
            .zip(work.previous.as_ref())
            .and_then(|(prefix, previous)| {
                (!work.kind.is_native())
                    .then(|| {
                        crate::domain::facts::append::Insertion::prefix(
                            &work.source,
                            prefix,
                            fact_limit,
                        )
                    })
                    .flatten()
                    .map(|source| {
                        crate::domain::facts::append::Tracker::new(
                            source,
                            previous.all,
                            fact_limit.min(65536),
                        )
                    })
            });
    let mut probe = work
        .prefix
        .as_ref()
        .filter(|_| !work.kind.is_native())
        .map(crate::domain::facts::append::Probe::new);
    let (fingerprints, mut facts, positions) = {
        let mut fingerprint = Fingerprinter::default();
        let window = fact_limit.min(65536);
        let mut positions = super::super::positions::ScopeEnds::new(window / 4);
        let mut replay = super::super::replay::Replay::new(window - window / 4);
        super::super::extraction::run(
            parser,
            &path,
            &work.source,
            work.kind,
            &events.cancel,
            &mut |language, mut fact: Fact| {
                if let Some(probe) = &mut probe {
                    probe.push(&fact);
                }
                if let Some(append) = &mut append {
                    append.push(language, &mut fact);
                }
                fingerprint.push(language, &mut fact);
                positions.push(&fact);
                replay.push(language, fact);
                Ok(())
            },
        )
        .map_err(|error| {
            if error.kind == ErrorKind::Limit {
                let code = if matches!(work.kind, super::super::events::InputKind::Configuration) {
                    "configuration_invalid"
                } else {
                    "source_analysis_limit"
                };
                Error::input(code, error.to_string(), None)
            } else {
                error
            }
        })?;
        (fingerprint.finish(), replay.finish(), positions.finish())
    };
    let mut preserve = false;
    if let Some(previous) = &work.previous {
        let next = &fingerprints;
        if next.all == previous.all
            && let Some(scope_ends) = positions
        {
            return events.send(Event::UnchangedFacts {
                partial: parser.partial,
                fragments: parser.fragments.clone(),
                native: parser.native_report(),
                record: work.record.clone(),
                span: SourceSpan::at(&work.source, 0, work.source.len()),
                scope_ends,
            });
        }
        preserve = next.environment == previous.environment;
    }
    let candidate = if parser.partial.is_none() {
        append::candidate(parser, work, events, fact_limit, append, probe, &mut facts)?
    } else {
        None
    };
    if facts.is_none() {
        parser.replay();
    }
    events.send(Event::Begin {
        record: work.record.clone(),
        partial: parser.partial,
        change: candidate.map(FileChange::Append).unwrap_or(if preserve {
            FileChange::Body
        } else {
            FileChange::Replace
        }),
    })?;
    if let Some(facts) = facts {
        if !facts.is_empty() {
            events.send(Event::Facts {
                path: Arc::clone(&path),
                facts,
            })?;
        }
        return events.send(Event::End {
            partial: parser.partial,
            path,
            fingerprints,
            native: parser.native_report(),
            fragments: parser.fragments.clone(),
        });
    }
    let mut fingerprint = Fingerprinter::default();
    let mut batch = super::super::batches::BatchSender::new(events, Arc::clone(&path), fact_limit);
    super::super::extraction::run(
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
        partial: parser.partial,
        fragments: parser.fragments.clone(),
        native: parser.native_report(),
        path,
        fingerprints: fingerprint.finish(),
    })
}
