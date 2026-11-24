// Purpose: native callee의 USR와 실제 선언 위치를 함께 보존하는 내부 계약.
// Pattern: Value Object.
// Usage: Clang projection이 기록하고 resolver가 가시성·충돌 경계 안에서 소비.
// Related: crates/kodstar-core/src/infrastructure/clang/projection.rs, crates/kodstar-core/src/application/analysis/resolve/clang.rs.

use std::borrow::Cow;

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Target<'a> {
    #[serde(borrow)]
    pub usr: Cow<'a, str>,
    #[serde(borrow)]
    pub file: Cow<'a, str>,
    pub position: usize,
    #[serde(borrow)]
    pub name: Cow<'a, str>,
}
