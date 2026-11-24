// Purpose: 재생 가능한 사실 또는 제한된 추가 추출로 삽입 재사용 후보를 검증.
// Pattern: Coordinator.
// Usage: worker 사전 검토가 writer에 Begin을 보내기 전에 호출.
// Related: crates/kodstar-core/src/application/analysis/workers/extraction.rs, crates/kodstar-core/src/domain/facts/append.rs.

use crate::{
    application::analysis::{
        events::{Events, Work},
        extraction,
    },
    domain::{
        diagnostics::Result,
        facts::{
            Fact,
            append::{Candidate, Probe, Tracker},
        },
        languages::Language,
    },
    infrastructure::parse::ParseSession,
};

pub(super) fn candidate(
    parser: &mut ParseSession,
    work: &Work,
    events: &Events,
    budget: usize,
    prefix: Option<Tracker>,
    probe: Option<Probe>,
    buffered: &mut Option<Vec<(Language, Fact)>>,
) -> Result<Option<Candidate>> {
    if let Some(prefix) = prefix {
        return Ok(prefix.finish());
    }
    let Some(source) = probe.and_then(|p| p.finish(&work.source, budget)) else {
        return Ok(None);
    };
    let Some(previous) = &work.previous else {
        return Ok(None);
    };
    let mut tracker = Tracker::new(source, previous.all, budget.min(65536));
    if let Some(facts) = buffered {
        for (language, fact) in facts {
            tracker.push(*language, fact);
        }
    } else {
        parser.replay();
        extraction::run(
            parser,
            &work.record.path,
            &work.source,
            work.kind,
            &events.cancel,
            &mut |language, mut fact| {
                tracker.push(language, &mut fact);
                Ok(())
            },
        )?;
    }
    Ok(tracker.finish())
}
