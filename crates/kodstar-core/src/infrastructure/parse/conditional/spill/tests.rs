// Purpose: spill의 해시 충돌 정확 대조와 디스크 예산 실패 계약 검증.
// Pattern: Unit Test.
// Usage: conditional spill adapter의 private SQLite 자원으로 실행.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/spill.rs.

use super::*;

#[test]
fn digest_collision_still_compares_every_key_byte() -> Result<()> {
    let cancel = Cancellation::default();
    let spill = Spill::new(8192)?;
    spill.insert(1, &"abcd", 9, &cancel)?;
    let (digest, _) = stream::digest(&"abce", &cancel)?;
    spill
        .connection
        .execute("UPDATE keys SET digest=?1", [digest.as_bytes().as_slice()])?;
    assert_eq!(spill.find(1, &"abce", &cancel)?, None);
    spill.insert(1, &"abce", 10, &cancel)?;
    assert_eq!(spill.find(1, &"abce", &cancel)?, Some(10));
    Ok(())
}

#[test]
fn chunk_write_exhaustion_is_a_limit_failure() -> Result<()> {
    let cancel = Cancellation::default();
    let spill = Spill::new(4096)?;
    let error = spill
        .insert(1, &"x".repeat(256 * 1024), 1, &cancel)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Limit);
    Ok(())
}
