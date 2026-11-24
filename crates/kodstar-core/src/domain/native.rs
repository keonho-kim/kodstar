// Purpose: 의미 사실과 독립적인 native 입력 관측·완료 진단 계약.
// Pattern: Value Objects.
// Usage: native adapter가 생성하고 분석 writer와 증분 선택이 소비.
// Related: crates/kodstar-core/src/infrastructure/clang/mod.rs, crates/kodstar-core/src/infrastructure/index/native.rs.

use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Observation {
    Missing,
    Directory,
    File([u8; 32]),
}

pub(crate) struct Report {
    pub environment: String,
    pub status: &'static str,
    /// A bounded diagnostic description; never includes source excerpts.
    pub detail: Option<String>,
    pub inputs: BTreeMap<String, Observation>,
}

impl Report {
    pub fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.environment.capacity()
            + self.detail.as_ref().map_or(0, String::capacity)
            + self
                .inputs
                .keys()
                .map(|path| path.capacity() + 128)
                .sum::<usize>()
    }
}
