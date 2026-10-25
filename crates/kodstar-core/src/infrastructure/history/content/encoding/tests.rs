// Purpose: 바이트 블록 인코딩의 복원·손상 거부·메모리 한도 검증.
// Pattern: Unit Test.
// Usage: 표준 Zstd·원문 dictionary와 손상 frame을 결정적 입력으로 대조.
// Related: crates/kodstar-core/src/infrastructure/history/content/encoding.rs.

use super::*;

#[test]
fn raw_and_delta_encodings_round_trip_binary_and_repeated_inputs() -> Result<()> {
    let mut encoder = Encoder::default();
    let mut decoder = Decoder::default();
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
            let encoded = encoder.encode(&original, &[])?;
            if encoded.len() <= MAX {
                assert_eq!(decoder.decode(&encoded, 1, length, &[])?, original);
            }
            let mut changed = original.clone();
            changed[length / 2] ^= 1;
            let encoded = encoder.encode(&changed, &original)?;
            if encoded.len() <= MAX {
                assert_eq!(decoder.decode(&encoded, 2, length, &original)?, changed);
            }
        }
    }
    Ok(())
}

#[test]
fn small_edits_use_bounded_zstd_dictionary_frames() -> Result<()> {
    let base = b"fn unchanged() {}\n".repeat(1000);
    let mut changed = b"// inserted\n".to_vec();
    changed.extend_from_slice(&base);
    changed[9000] = b'X';
    let encoded = Encoder::default().encode(&changed, &base)?;
    assert!(encoded.len() < 100);
    assert_eq!(
        Decoder::default().decode(&encoded, 2, changed.len(), &base)?,
        changed
    );
    Ok(())
}

#[test]
fn corrupt_frames_and_declared_bounds_are_rejected() {
    let mut decoder = Decoder::default();
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
        assert!(decoder.decode(&bytes, codec, size, &base).is_err());
    }
}

#[test]
fn arbitrary_short_payloads_never_escape_bounds_or_panic() {
    let mut decoder = Decoder::default();
    for first in 0..=255u8 {
        for second in 0..=255u8 {
            if let Ok(bytes) = decoder.decode(&[first, second], 1, 32, &[]) {
                assert_eq!(bytes.len(), 32);
            }
        }
    }
}

#[test]
fn standard_frames_and_dictionary_magic_source_bytes_round_trip() -> Result<()> {
    let mut encoder = Encoder::default();
    let mut decoder = Decoder::default();
    let mut base = b"fn repeated() { repeated(); }\n".repeat(2000);
    base[..4].copy_from_slice(&[0x37, 0xa4, 0x30, 0xec]);
    let mut changed = base.clone();
    changed[456] ^= 1;
    let encoded = encoder.encode(&changed, &base)?;
    assert_eq!(decoder.decode(&encoded, 2, changed.len(), &base)?, changed);
    let mut reference = zstd::bulk::Decompressor::with_dictionary(dictionary(&base))
        .map_err(|e| Error::caused_by("reference decoder", e))?;
    assert_eq!(
        reference
            .decompress(&encoded, changed.len())
            .map_err(|e| Error::caused_by("reference decode", e))?,
        changed
    );
    let independent = encoder.encode(&changed, &[])?;
    assert_eq!(
        zstd::bulk::decompress(&independent, changed.len())
            .map_err(|e| Error::caused_by("reference decode", e))?,
        changed
    );
    Ok(())
}

#[test]
fn extra_frames_truncation_size_and_excess_window_are_rejected() -> Result<()> {
    let original = vec![b'a'; 4096];
    let encoded = Encoder::default().encode(&original, &[])?;
    let mut decoder = Decoder::default();
    for length in 0..encoded.len() {
        assert!(
            decoder
                .decode(&encoded[..length], 1, original.len(), &[])
                .is_err()
        );
    }
    for extra in [vec![0], encoded.clone()] {
        let appended = [encoded.as_slice(), &extra].concat();
        assert!(decoder.decode(&appended, 1, original.len(), &[]).is_err());
    }
    assert!(
        decoder
            .decode(&encoded, 1, original.len() - 1, &[])
            .is_err()
    );
    // Valid single raw block and declared output, but a window far beyond 64KiB.
    let mut excessive = vec![0x28, 0xb5, 0x2f, 0xfd, 0x80, 0x40];
    excessive.extend_from_slice(&32u32.to_le_bytes());
    excessive.extend_from_slice(&[1, 1, 0]);
    excessive.extend_from_slice(&[b'a'; 32]);
    assert_eq!(
        zstd_safe::get_frame_content_size(&excessive).ok(),
        Some(Some(32))
    );
    assert!(decoder.decode(&excessive, 1, 32, &[]).is_err());
    let mut checked =
        zstd::bulk::Compressor::new(1).map_err(|e| Error::caused_by("reference compressor", e))?;
    checked
        .set_parameter(CParameter::ChecksumFlag(true))
        .map_err(|e| Error::caused_by("reference checksum", e))?;
    let mut corrupt = checked
        .compress(&original)
        .map_err(|e| Error::caused_by("reference compression", e))?;
    *corrupt.last_mut().expect("nonempty Zstd frame") ^= 1;
    assert!(decoder.decode(&corrupt, 1, original.len(), &[]).is_err());
    assert_eq!(decoder.decode(&encoded, 1, original.len(), &[])?, original);
    assert!(Encoder::default().encode(&vec![0; MAX + 1], &[]).is_err());
    Ok(())
}
