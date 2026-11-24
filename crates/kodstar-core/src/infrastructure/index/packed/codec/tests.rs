// Purpose: 전체 u64 범위와 잘못된 정수·문자열 경계의 codec 성질 검증.
// Pattern: Property Test.
// Usage: cargo test -p kodstar-core packed::codec::tests.
// Related: crates/kodstar-core/src/infrastructure/index/packed/codec.rs.

use super::*;
use proptest::{prelude::*, test_runner::RngSeed};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(0x6b6f6473746172),
        ..ProptestConfig::default()
    })]

    #[test]
    fn full_width_integers_and_unicode_keep_exact_boundaries(
        values in prop::collection::vec(any::<u64>(), 0..128),
        chars in prop::collection::vec(any::<char>(), 0..256),
    ) {
        let original: String = chars.into_iter().collect();
        let mut bytes = Vec::new();
        for value in &values {
            integer(&mut bytes, *value);
        }
        text(&mut bytes, &original);
        let mut input = Input(&bytes);
        for value in values {
            prop_assert_eq!(input.integer().unwrap(), value);
        }
        prop_assert_eq!(input.text_ref().unwrap(), original.as_str());
        prop_assert!(input.finish().is_ok());
    }

    #[test]
    fn accepted_integers_have_a_unique_canonical_encoding(
        bytes in prop::collection::vec(any::<u8>(), 0..32),
    ) {
        let mut input = Input(&bytes);
        if let Ok(value) = input.integer() {
            let consumed = bytes.len() - input.0.len();
            let mut canonical = Vec::new();
            integer(&mut canonical, value);
            prop_assert_eq!(canonical.as_slice(), &bytes[..consumed]);
        }
        let mut input = Input(&bytes);
        if let Ok(text) = input.text_ref() {
            let consumed = bytes.len() - input.0.len();
            let mut canonical = Vec::new();
            super::text(&mut canonical, text);
            prop_assert_eq!(canonical.as_slice(), &bytes[..consumed]);
        }
    }
}

#[test]
fn overflow_truncation_overlong_encoding_and_trailing_bytes_are_rejected() {
    for invalid in [vec![0x80], vec![0x80, 0], vec![0xff; 10], vec![0x80; 11]] {
        assert!(Input(&invalid).integer().is_err());
    }
    assert!(Input(&[1, 0xff]).text_ref().is_err());
    assert!(Input(&[2, b'a']).text_ref().is_err());
    assert!(Input(&[0]).finish().is_err());
    let mut bytes = Vec::new();
    integer(&mut bytes, i64::MAX as u64 + 1);
    assert!(Input(&bytes).id().is_err());
}

#[test]
fn ten_byte_terminals_preserve_u64_max_and_reject_overflow() {
    let mut encoded = [0xff; 10];
    encoded[9] = 1;
    let mut input = Input(&encoded);
    assert_eq!(input.integer().unwrap(), u64::MAX);
    assert!(input.finish().is_ok());
    for terminal in [0, 2, 3, 0x7f, 0x80, 0xff] {
        encoded[9] = terminal;
        assert!(Input(&encoded).integer().is_err(), "terminal {terminal}");
    }
}
