// Purpose: 분석 단계의 내장 헤더 복원 context 재사용과 원문 버퍼 수명 소유.
// Pattern: Resource Adapter.
// Usage: native VFS callback이 단계 HeaderDecoder에서 고정 리소스를 요청.
// Related: crates/kodstar-clang/build/headers.rs, crates/kodstar-clang/src/callbacks.rs.

use super::{InputError, Source};
use std::sync::Mutex;
static INDEX: &[(&str, usize, usize, usize)] = include!(concat!(env!("OUT_DIR"), "/headers.rs"));
static PACK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/headers.pack"));
static DICTIONARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/headers.dict"));

/// Reusable decoder owned by one analysis phase; retains no source payloads.
#[derive(Default)]
pub struct HeaderDecoder(Mutex<Option<zstd::bulk::Decompressor<'static>>>);

impl HeaderDecoder {
    fn bytes(&self, index: usize) -> Result<Vec<u8>, InputError> {
        let (_, offset, length, raw) = INDEX[index];
        let mut context = self.0.lock().map_err(|_| InputError::Failure)?;
        if context.is_none() {
            *context = Some(
                zstd::bulk::Decompressor::with_dictionary(DICTIONARY)
                    .map_err(|_| InputError::Failure)?,
            );
        }
        let mut output = Vec::with_capacity(raw + 1);
        context
            .as_mut()
            .ok_or(InputError::Failure)?
            .decompress_to_buffer(&PACK[offset..offset + length], &mut output)
            .map_err(|_| InputError::Failure)?;
        Ok(output)
    }
}

pub(super) fn read(
    headers: &HeaderDecoder,
    path: &str,
    content: bool,
) -> Result<Source, InputError> {
    let path = path
        .strip_prefix("/__kodstar_clang")
        .ok_or(InputError::Missing)?;
    let name = path.strip_prefix('/').unwrap_or(path);
    if let Ok(index) = INDEX.binary_search_by_key(&name, |h| h.0) {
        let (_, _, _, raw) = INDEX[index];
        let bytes = if content {
            Some(headers.bytes(index)?)
        } else {
            None
        };
        return Ok(Source::File { size: raw, bytes });
    }
    let prefix = format!("{}/", name.trim_end_matches('/'));
    // Search the directory prefix itself: a sibling `_types.h` sorts before
    // `_types/` and must not hide all of that directory's descendants.
    let index = INDEX.partition_point(|h| h.0 < prefix.as_str());
    if name.is_empty() || INDEX.get(index).is_some_and(|h| h.0.starts_with(&prefix)) {
        return Ok(Source::Directory);
    }
    Err(InputError::Missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_decoder_preserves_every_header_without_retaining_payloads() {
        let resources = HeaderDecoder::default();
        let index = INDEX
            .iter()
            .position(|(name, ..)| *name == "include/stddef.h")
            .unwrap();
        let expected = resources.bytes(index).unwrap();
        assert_eq!(resources.bytes(index).unwrap(), expected);
        assert!(resources.0.lock().unwrap().is_some());
    }
}
