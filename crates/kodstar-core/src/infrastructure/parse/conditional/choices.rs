// Purpose: 필요한 코드 coverage의 투영만 제한된 횟수·메모리에서 지연 생성.
// Pattern: Bounded Candidate Stream.
// Usage: parser가 성공한 coverage를 갱신하며 다음 입력을 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/mod.rs, crates/kodstar-core/src/infrastructure/parse/conditional/coverage.rs.

use super::{Plan, Projection, Values, coverage::Coverage, limit, mask};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use std::collections::HashSet;

pub(in crate::infrastructure::parse) struct Choices<'a> {
    plan: &'a Plan,
    source: &'a [u8],
    cancel: &'a Cancellation,
    position: usize,
    seen: HashSet<Vec<bool>>,
    bytes: usize,
}

impl<'a> Choices<'a> {
    pub(super) fn new(plan: &'a Plan, source: &'a [u8], cancel: &'a Cancellation) -> Self {
        Self {
            plan,
            source,
            cancel,
            position: 0,
            seen: HashSet::new(),
            bytes: plan.name_bytes.saturating_mul(2),
        }
    }

    pub(in crate::infrastructure::parse) fn next_projection(
        &mut self,
        coverage: &Coverage,
        parsed: bool,
    ) -> Result<Option<Projection<'a>>> {
        if parsed && coverage.complete() {
            return Ok(None);
        }
        let count = self.plan.regions.len();
        while self.position < 2 + count.saturating_mul(2) {
            self.cancel.check()?;
            let position = self.position;
            self.position += 1;
            let Some(complete) = self.plan.assignment(position, self.cancel)? else {
                continue;
            };
            if self.seen.contains(&complete) {
                continue;
            }
            let values: Values = self
                .plan
                .variables
                .iter()
                .cloned()
                .zip(complete.iter().copied())
                .collect();
            if parsed && !coverage.needs(self.plan, &values) {
                continue;
            }
            self.bytes = self
                .bytes
                .saturating_add(complete.len().saturating_add(64).saturating_mul(2));
            if self.seen.len() >= 256 || self.bytes > self.plan.budget {
                return Err(limit("projection count or bytes"));
            }
            self.seen.insert(complete);
            return Ok(Some(self.plan.project(self.source, position, values)));
        }
        Ok(None)
    }
}

impl Plan {
    fn assignment(&self, position: usize, cancel: &Cancellation) -> Result<Option<Vec<bool>>> {
        let complete = if position < 2 {
            vec![position == 1; self.variables.len()]
        } else {
            let index = position - 2;
            let Some(values) = self.regions[index / 2].guard.witness(cancel)? else {
                return Ok(None);
            };
            self.variables
                .iter()
                .map(|key| values.get(key).copied().unwrap_or(index % 2 == 1))
                .collect()
        };
        Ok(Some(complete))
    }

    pub(super) fn projection(
        &self,
        source: &[u8],
        position: usize,
        cancel: &Cancellation,
    ) -> Result<Projection<'_>> {
        let complete = self.assignment(position, cancel)?.ok_or_else(|| {
            crate::domain::diagnostics::Error::operation(
                "conditional witness is no longer reachable",
            )
        })?;
        let values = self.variables.iter().cloned().zip(complete).collect();
        Ok(self.project(source, position, values))
    }

    fn project(&self, source: &[u8], position: usize, values: Values) -> Projection<'_> {
        let mut projected = source.to_vec();
        for range in &self.directives {
            mask(&mut projected[range.clone()]);
        }
        for region in &self.regions {
            if region.guard.evaluate(&values) != Some(true) {
                mask(&mut projected[region.range.clone()]);
            }
        }
        Projection {
            source: projected,
            plan: self,
            values,
            position,
        }
    }
}
