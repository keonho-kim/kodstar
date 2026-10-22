// Purpose: 엔진 use case와 실행 정책의 내부 모듈 구성.
// Pattern: Simple Module.
// Usage: core 공개 API가 선택한 실행 경계를 사용.
// Related: crates/kodstar-core/src/lib.rs.

//! Engine use cases and resource ownership; composes domain rules with concrete adapters.

pub(crate) mod analysis;
pub(crate) mod cache;
pub(crate) mod engine;
pub(crate) mod execution;
pub(crate) mod history;
pub(crate) mod installation;
pub(crate) mod maintenance;
pub(crate) mod query;
pub(crate) mod workspace;
