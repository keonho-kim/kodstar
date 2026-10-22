// Purpose: 바이트 블록 인코딩의 복원·손상 거부·메모리 한도 검증.
// Pattern: Unit Test.
// Usage: 원문·델타 표현을 결정적 입력과 잘못된 instruction으로 대조.
// Related: crates/kodstar-core/src/infrastructure/history/content/encoding.rs.

use super::*;

#[test]
fn raw_and_delta_encodings_round_trip_binary_and_repeated_inputs() -> Result<()> {
    let mut random = 42u64;
    for length in [1, 3, 4, 17, 128, 2048, 8192, MAX] {
        let bytes: Vec<u8> = (0..length)
            .map(|_| {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                random as u8
            })
            .collect();
        for original in [
            bytes,
            vec![b'a'; length],
            b"fn repeated() { repeated(); }\n"
                .iter()
                .cycle()
                .copied()
                .take(length)
                .collect(),
        ] {
            let encoded = encode(&original, &[]);
            if encoded.len() <= MAX {
                assert_eq!(decode(&encoded, 1, length, &[])?, original);
            }
            let mut changed = original.clone();
            changed[length / 2] ^= 1;
            let encoded = encode(&changed, &original);
            if encoded.len() <= MAX {
                assert_eq!(decode(&encoded, 2, length, &original)?, changed);
            }
        }
    }
    Ok(())
}

#[test]
fn small_edits_produce_bounded_copy_instructions() -> Result<()> {
    let base = b"fn unchanged() {}\n".repeat(1000);
    let mut changed = b"// inserted\n".to_vec();
    changed.extend_from_slice(&base);
    changed[9000] = b'X';
    let encoded = encode(&changed, &base);
    assert!(encoded.len() < 100);
    assert_eq!(decode(&encoded, 2, changed.len(), &base)?, changed);
    Ok(())
}

#[test]
fn corrupt_copy_literal_overflow_and_declared_bounds_are_rejected() {
    for (bytes, codec, size, base) in [
        (vec![1, 0], 1, 4, vec![]),
        (vec![0], 1, 1, vec![]),
        (vec![255; 11], 1, 1, vec![]),
        (vec![0x80, 0, 0], 1, 1, vec![]),
        (vec![0, 1], 0, 1, vec![]),
        (vec![0, 1], 1, MAX + 1, vec![]),
        (vec![0, 1], 2, 1, vec![]),
        (vec![1, 1], 2, 4, vec![1]),
    ] {
        assert!(decode(&bytes, codec, size, &base).is_err());
    }
}

#[test]
fn arbitrary_short_payloads_never_escape_bounds_or_panic() {
    for first in 0..=255u8 {
        for second in 0..=255u8 {
            if let Ok(bytes) = decode(&[first, second], 1, 32, &[]) {
                assert_eq!(bytes.len(), 32);
            }
        }
    }
}
