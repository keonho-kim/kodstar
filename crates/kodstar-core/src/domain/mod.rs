// Purpose: 순수 도메인 값과 규칙의 내부 모듈 구성.
// Pattern: Simple Module.
// Usage: core의 domain 경계 선언.
// Related: crates/kodstar-core/src/lib.rs.

//! Analysis concepts and rules independent of filesystem, parser, and storage adapters.

pub(crate) mod cancellation;
pub(crate) mod diagnostics;
pub(crate) mod facts;
pub(crate) mod languages;
pub(crate) mod query;
pub(crate) mod relations;
pub(crate) mod resolve;
pub(crate) mod results;
pub(crate) mod source;
pub(crate) mod symbols;
