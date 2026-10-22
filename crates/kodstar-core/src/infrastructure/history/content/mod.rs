// Purpose: 영구 원문 내용의 주소·분할·인코딩·검증된 읽기 경계.
// Pattern: Content Store.
// Usage: history capture·diff·복원·유지보수가 이 기능 모듈을 사용.
// Related: crates/kodstar-core/src/infrastructure/history/content/blobs.rs, crates/kodstar-core/src/infrastructure/history/content/chunks.rs.

pub(crate) mod blobs;
pub(crate) mod chunk_ranges;
pub(super) mod chunk_read;
mod chunking;
pub(super) mod chunks;
pub(super) mod encoding;
pub(crate) mod reader;
