// Purpose: 구문이 성립한 투영이 모든 도달 가능한 코드 영역을 실제로 포함했는지 확인.
// Pattern: Coverage Ledger.
// Usage: parser가 투영 추출 전에 필요 영역을 확인하고 성공한 tree만 기록.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/mod.rs.

use super::{Plan, Projection, Values};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;

pub(in crate::infrastructure::parse) struct Coverage {
    remaining: Vec<bool>,
}

impl Plan {
    pub(in crate::infrastructure::parse) fn coverage(
        &self,
        source: &[u8],
        cancel: &Cancellation,
    ) -> Result<Coverage> {
        let mut remaining = Vec::with_capacity(self.regions.len());
        for region in &self.regions {
            remaining.push(
                has_code(&source[region.range.clone()]) && region.guard.witness(cancel)?.is_some(),
            );
        }
        Ok(Coverage { remaining })
    }
}

impl Coverage {
    pub(in crate::infrastructure::parse) fn complete(&self) -> bool {
        !self.remaining.iter().any(|needed| *needed)
    }

    pub(in crate::infrastructure::parse) fn first_uncovered(
        &self,
        plan: &Plan,
    ) -> Option<std::ops::Range<usize>> {
        self.remaining
            .iter()
            .zip(&plan.regions)
            .find_map(|(needed, region)| needed.then(|| region.range.clone()))
    }

    pub(super) fn needs(&self, plan: &Plan, values: &Values) -> bool {
        self.remaining
            .iter()
            .zip(&plan.regions)
            .any(|(needed, region)| *needed && region.guard.evaluate(values) == Some(true))
    }

    pub(in crate::infrastructure::parse) fn record(&mut self, projection: &Projection<'_>) {
        for (needed, region) in self.remaining.iter_mut().zip(&projection.plan.regions) {
            if region.guard.evaluate(&projection.values) == Some(true) {
                *needed = false;
            }
        }
    }
}

fn has_code(source: &[u8]) -> bool {
    let mut at = 0;
    while at < source.len() {
        if source[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let rest = &source[at..];
        if rest.starts_with(b"//") {
            at += memchr::memchr(b'\n', rest).unwrap_or(rest.len());
            continue;
        }
        let (width, close): (usize, &[u8]) = if rest.starts_with(b"/*") {
            (2, b"*/")
        } else {
            return true;
        };
        let Some(length) = memchr::memmem::find(&rest[width..], close) else {
            return true; // Unterminated trivia must still reach syntax validation.
        };
        at += width + length + close.len();
    }
    false
}
