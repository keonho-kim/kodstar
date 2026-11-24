// Purpose: 구체 I/O·parser·저장 adapter의 내부 모듈 구성.
// Pattern: Simple Module.
// Usage: application이 필요한 adapter를 직접 사용.
// Related: crates/kodstar-core/src/lib.rs.

//! Concrete I/O and parser adapters. Analysis policy belongs to the inner layers.

pub(crate) mod cache;
pub(crate) mod clang;
pub(crate) mod formats;
pub(crate) mod history;
pub(crate) mod index;
pub(crate) mod installation;
pub(crate) mod languages;
pub(crate) mod parse;
pub(crate) mod scan;
pub(crate) mod watch;
