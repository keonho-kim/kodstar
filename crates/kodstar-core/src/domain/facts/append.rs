// Purpose: 추가 선언의 후보와 기존·신규 사실의 위치 분류 계약.
// Pattern: Value Objects.
// Usage: worker의 순수 증명과 index 행 보존이 동일한 추가 구간을 사용.
// Related: crates/kodstar-core/src/domain/facts/append/source.rs, crates/kodstar-core/src/domain/facts/append/facts.rs.

mod facts;
mod source;
use super::Fact;
use crate::domain::source::SourceSpan;
pub(crate) use facts::Tracker;
pub(crate) use source::{Insertion, Prefix, Probe};
use std::ops::Range;

pub(crate) struct Candidate {
    pub range: Range<usize>,
    pub names: Vec<String>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Membership {
    Existing,
    Added,
    Invalid,
}

pub(crate) fn span(fact: &Fact) -> Option<SourceSpan> {
    Some(match fact {
        Fact::Definition(d) => d.span,
        Fact::Scope(s) => s.span,
        Fact::Binding(b) => b.span,
        Fact::Use(u) => u.span,
        Fact::Module { .. } => return None,
    })
}
pub(crate) fn set_span(fact: &mut Fact, span: SourceSpan) {
    match fact {
        Fact::Definition(d) => d.span = span,
        Fact::Scope(s) => s.span = span,
        Fact::Binding(b) => b.span = span,
        Fact::Use(u) => u.span = span,
        Fact::Module { .. } => {}
    }
}
pub(crate) fn membership(fact: &Fact, range: &Range<usize>) -> Membership {
    if matches!(fact, Fact::Scope(s) if s.local_id == 0)
        || matches!(fact, Fact::Definition(d) if d.local_id == 0)
    {
        return Membership::Existing;
    }
    let Some(span) = span(fact) else {
        return Membership::Existing;
    };
    if span.end_byte <= range.start || span.start_byte >= range.end {
        return Membership::Existing;
    }
    if span.start_byte >= range.start && span.end_byte <= range.end {
        return Membership::Added;
    }
    if span.start_byte < range.start && span.end_byte >= range.end {
        return Membership::Existing;
    }
    Membership::Invalid
}

pub(crate) fn original_bytes(span: SourceSpan, range: &Range<usize>) -> (usize, usize) {
    let delta = range.len();
    (
        if span.start_byte >= range.end {
            span.start_byte - delta
        } else {
            span.start_byte
        },
        if span.end_byte >= range.end {
            span.end_byte - delta
        } else {
            span.end_byte
        },
    )
}
