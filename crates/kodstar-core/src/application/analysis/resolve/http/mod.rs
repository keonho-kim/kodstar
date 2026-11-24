// Purpose: 검증된 HTTP 등록의 임시 인덱스와 요청·응답 해석 진입점.
// Pattern: Resolution Adapter.
// Usage: resolve가 세대 준비 후 호출하고 fact 해석에서 소비.
// Related: crates/kodstar-core/src/application/analysis/resolve/http/origin.rs, crates/kodstar-core/src/application/analysis/resolve/http/requests.rs.

mod exports;
mod origin;
mod requests;
mod routes;

pub(super) use routes::prepare;
