// Purpose: 작은 변경 파일의 첫 추출 사실을 한 배치 예산 안에서 재사용.
// Pattern: Bounded Buffer.
// Usage: worker가 fingerprint 계산 중 소유하고 판정 뒤 배치로 이전하거나 해제.
// Related: crates/kodstar-core/src/application/analysis/workers.rs, crates/kodstar-core/src/application/analysis/batches.rs.

use crate::domain::{facts::Fact, languages::Language};

pub(super) struct Replay {
    facts: Option<Vec<(Language, Fact)>>,
    bytes: usize,
    limit: usize,
}

impl Replay {
    pub fn new(limit: usize) -> Self {
        Self {
            facts: Some(Vec::new()),
            bytes: 0,
            limit: limit.min(65536),
        }
    }

    pub fn push(&mut self, language: Language, fact: Fact) {
        let Some(facts) = &mut self.facts else {
            return;
        };
        let extra = fact.owned_bytes() - std::mem::size_of::<Fact>();
        let row = std::mem::size_of::<(Language, Fact)>();
        if facts.capacity() == 0 {
            let capacity = (self.limit.saturating_sub(extra) / row).min(128);
            if capacity == 0 {
                self.facts = None;
                return;
            }
            *facts = Vec::with_capacity(capacity);
            self.bytes = facts.capacity() * row;
        }
        if facts.len() == facts.capacity() || self.bytes.saturating_add(extra) > self.limit {
            self.facts = None;
            return;
        }
        self.bytes += extra;
        facts.push((language, fact));
    }

    pub fn finish(self) -> Option<Vec<(Language, Fact)>> {
        self.facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_inputs_release_the_buffer_and_use_the_existing_streaming_path() {
        let mut replay = Replay::new(1024);
        replay.push(
            Language::Go,
            Fact::Module {
                name: "api".into(),
                declaration_only: false,
            },
        );
        replay.push(
            Language::Go,
            Fact::Module {
                name: "x".repeat(2048),
                declaration_only: false,
            },
        );
        replay.push(
            Language::Go,
            Fact::Module {
                name: "later".into(),
                declaration_only: false,
            },
        );
        assert!(replay.finish().is_none());
    }

    #[test]
    fn small_inputs_preserve_fact_order_and_owned_byte_limits() {
        let mut replay = Replay::new(65536);
        for name in ["first", "second"] {
            replay.push(
                Language::Go,
                Fact::Module {
                    name: name.into(),
                    declaration_only: false,
                },
            );
        }
        let facts = replay.finish().expect("bounded fixture");
        let extra: usize = facts
            .iter()
            .map(|(_, f)| f.owned_bytes() - std::mem::size_of::<Fact>())
            .sum();
        assert!(facts.capacity() * std::mem::size_of::<(Language, Fact)>() + extra <= 65536);
        assert!(matches!(&facts[0].1, Fact::Module { name, .. } if name=="first"));
        assert!(matches!(&facts[1].1, Fact::Module { name, .. } if name=="second"));
    }
}
