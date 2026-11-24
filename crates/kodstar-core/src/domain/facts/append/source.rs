// Purpose: 단일 삽입을 제거한 바이트 해시와 원본 위치 색인으로 이전 입력을 증명.
// Pattern: Streaming Digest / Immutable Index.
// Usage: worker가 마지막 선언 후보를 관찰하고 증명한 삽입 구간을 요청.
// Related: crates/kodstar-core/src/domain/facts/append.rs, crates/kodstar-core/src/domain/source/lines.rs.

use super::{Fact, span};
use crate::domain::source::{SourceSpan, lines::Lines};
use std::{collections::VecDeque, ops::Range};

#[derive(Clone)]
pub(crate) struct Prefix {
    pub bytes: usize,
    pub hash: [u8; 32],
}
pub(crate) struct Insertion {
    pub range: Range<usize>,
    lines: Lines,
}
impl Insertion {
    pub fn prefix(source: &[u8], prior: &Prefix, budget: usize) -> Option<Self> {
        Self::verify(source, prior, prior.bytes..source.len(), budget)
    }
    fn verify(source: &[u8], prior: &Prefix, range: Range<usize>, budget: usize) -> Option<Self> {
        if range.is_empty() || range.end > source.len() || source.len() - range.len() != prior.bytes
        {
            return None;
        }
        let mut hash = blake3::Hasher::new();
        hash.update(&source[..range.start]);
        hash.update(&source[range.end..]);
        if hash.finalize().as_bytes() != &prior.hash {
            return None;
        }
        let lines = Lines::joined(&source[..range.start], &source[range.end..], budget)?;
        Some(Self { range, lines })
    }
    pub fn original(&self, span: SourceSpan) -> SourceSpan {
        let (start, end) = super::original_bytes(span, &self.range);
        self.lines.span(start, end)
    }
}

pub(crate) struct Probe {
    prior: Prefix,
    ends: VecDeque<usize>,
}
impl Probe {
    pub fn new(prior: &Prefix) -> Self {
        Self {
            prior: prior.clone(),
            ends: VecDeque::with_capacity(8),
        }
    }
    pub fn push(&mut self, fact: &Fact) {
        if matches!(fact, Fact::Definition(d) if d.local_id != 0)
            && let Some(span) = span(fact)
        {
            if self.ends.len() == 8 {
                self.ends.pop_front();
            }
            self.ends.push_back(span.end_byte);
        }
    }
    pub fn finish(self, source: &[u8], budget: usize) -> Option<Insertion> {
        let delta = source.len().checked_sub(self.prior.bytes)?;
        if delta == 0 {
            return None;
        }
        for end in self.ends.into_iter().rev() {
            for extra in 0..=32 {
                let end = end.checked_add(extra)?;
                if end > source.len() || extra > 0 && !source[end - 1].is_ascii_whitespace() {
                    break;
                }
                if end >= delta
                    && let Some(proof) =
                        Insertion::verify(source, &self.prior, end - delta..end, budget)
                {
                    return Some(proof);
                }
            }
        }
        None
    }
}
