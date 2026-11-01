// Purpose: 관계를 재사용할 파일의 scope 끝 위치를 제한된 worker 예산 안에 수집.
// Pattern: Bounded Buffer.
// Usage: worker가 fingerprint와 함께 수집해 UnchangedFacts의 writer로 이전.
// Related: crates/kodstar-core/src/application/analysis/workers.rs, crates/kodstar-core/src/infrastructure/index/write.rs.

use crate::domain::facts::Fact;

pub(super) struct ScopeEnds(Option<Vec<(u32, usize)>>);

impl ScopeEnds {
    pub fn new(limit: usize) -> Self {
        Self(Some(Vec::with_capacity(
            limit / std::mem::size_of::<(u32, usize)>(),
        )))
    }

    pub fn push(&mut self, fact: &Fact) {
        let Fact::Scope(scope) = fact else { return };
        if scope.local_id == 0 {
            return;
        }
        let Some(ends) = &mut self.0 else { return };
        if ends.len() == ends.capacity() {
            self.0 = None;
            return;
        }
        ends.push((scope.local_id, scope.span.end_byte));
    }

    pub fn finish(self) -> Option<Vec<(u32, usize)>> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{facts::Scope, source::SourceSpan};

    #[test]
    fn exhausting_the_position_budget_releases_it_and_requires_normal_refresh() {
        let mut ends = ScopeEnds::new(std::mem::size_of::<(u32, usize)>());
        for local_id in 0..3 {
            ends.push(&Fact::Scope(Scope {
                local_id,
                parent: None,
                span: SourceSpan::at(b"scope", 0, 5),
            }));
        }
        assert!(ends.finish().is_none());
    }
}
