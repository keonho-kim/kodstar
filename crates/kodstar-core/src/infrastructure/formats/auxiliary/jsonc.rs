// Purpose: JSONC trailing comma를 원본 byte 위치를 보존하며 masking.
// Pattern: Simple Module.
// Usage: 설정 adapter가 공식 JSON grammar에 넘길 입력을 준비.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/configuration.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::borrow::Cow;

pub(super) fn input(source: &[u8]) -> Result<Cow<'_, [u8]>> {
    let mut masked = None;
    let (mut position, mut quoted, mut escaped) = (0, false, false);
    let mut depth = 0usize;
    while position < source.len() {
        let byte = source[position];
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else if byte == b'"' {
            quoted = true;
        } else if source.get(position..position + 2) == Some(b"//") {
            position = source[position..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(source.len(), |n| position + n);
            continue;
        } else if source.get(position..position + 2) == Some(b"/*") {
            position = block_end(source, position + 2);
            continue;
        } else if matches!(byte, b'{' | b'[') {
            depth += 1;
            if depth > 512 {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "configuration nesting exceeds 512 levels",
                ));
            }
        } else if matches!(byte, b'}' | b']') {
            depth = depth.saturating_sub(1);
        } else if byte == b',' && matches!(significant(source, position + 1), Some(b'}' | b']')) {
            let bytes = masked.get_or_insert_with(|| source.to_vec());
            bytes[position] = b' ';
        }
        position += 1;
    }
    Ok(masked.map_or(Cow::Borrowed(source), Cow::Owned))
}

fn significant(source: &[u8], mut position: usize) -> Option<u8> {
    loop {
        let byte = *source.get(position)?;
        if byte.is_ascii_whitespace() {
            position += 1;
        } else if source.get(position..position + 2) == Some(b"//") {
            position = source[position..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(source.len(), |n| position + n);
        } else if source.get(position..position + 2) == Some(b"/*") {
            position = block_end(source, position + 2);
        } else {
            return Some(byte);
        }
    }
}

fn block_end(source: &[u8], position: usize) -> usize {
    source[position..]
        .windows(2)
        .position(|pair| pair == b"*/")
        .map_or(source.len(), |n| position + n + 2)
}
