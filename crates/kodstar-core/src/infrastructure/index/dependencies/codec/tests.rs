// Purpose: 성공 bit·전체 fact ID·손상 입력의 lookup codec 성질 검증.
// Pattern: Property Test.
// Usage: cargo test -p kodstar-core dependencies::codec::tests.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs.

use super::*;
use proptest::{prelude::*, test_runner::RngSeed};
use std::collections::BTreeMap;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(0x706f7374696e67),
        ..ProptestConfig::default()
    })]

    #[test]
    fn sparse_full_width_ids_and_found_bits_roundtrip(
        pairs in prop::collection::vec((1i64..=i64::MAX, any::<bool>()), 1..=MAX_COUNT),
    ) {
        let pairs: Vec<_> = pairs.into_iter()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect();
        let mut bytes = Vec::new();
        let mut previous = 0;
        for &(id, found) in &pairs {
            push(&mut bytes, id, previous, found).unwrap();
            previous = id;
        }
        let mut actual = Vec::new();
        visit(&bytes, pairs.len(), |id, found| {
            actual.push((id, found));
            Ok(())
        }).unwrap();
        prop_assert_eq!(actual.as_slice(), pairs.as_slice());
        bytes.push(0);
        prop_assert!(visit(&bytes, pairs.len(), |_, _| Ok(())).is_err());
    }

    #[test]
    fn arbitrary_accepted_blocks_are_canonical_and_strictly_increasing(
        bytes in prop::collection::vec(any::<u8>(), 0..256),
        count in 0usize..=129,
    ) {
        let mut pairs = Vec::new();
        let outcome = visit(&bytes, count, |id, found| {
            pairs.push((id, found));
            Ok(())
        });
        if outcome.is_ok() {
            prop_assert_eq!(pairs.len(), count);
            let mut encoded = Vec::new();
            let mut previous = 0;
            for (id, found) in pairs {
                prop_assert!(id > previous);
                push(&mut encoded, id, previous, found).unwrap();
                previous = id;
            }
            prop_assert_eq!(encoded, bytes);
        }
    }
}
