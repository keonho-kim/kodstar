// Purpose: 제한된 Zstd frame과 깊이 1 원문 dictionary의 압축·복원.
// Pattern: Bounded Codec Adapter.
// Usage: 쓰기·읽기 작업이 context를 소유하고 청크 사이에 재사용.
// Related: crates/kodstar-core/src/infrastructure/history/content/chunks.rs, crates/kodstar-core/src/infrastructure/history/content/chunk_read.rs.

use crate::domain::diagnostics::{Error, Result};
use zstd::zstd_safe::{
    self, CCtx, CParameter, DCtx, DParameter, InBuffer, OutBuffer, ResetDirective,
};

#[cfg(test)]
mod tests;

const WINDOW_LOG: u32 = 16;
pub(in crate::infrastructure::history) const MAX: usize = 1 << WINDOW_LOG;
pub(in crate::infrastructure::history) const MIN_INPUT: usize = 128;
pub(in crate::infrastructure::history) const SAVINGS: usize = 32;

#[derive(Default)]
pub(in crate::infrastructure::history) struct Encoder {
    context: Option<CCtx<'static>>,
}

impl Encoder {
    pub fn encode(&mut self, input: &[u8], base: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() || input.len() > MAX || base.len() > MAX {
            return Err(invalid());
        }
        let context = match &mut self.context {
            Some(context) => context,
            slot @ None => {
                let mut context = CCtx::try_create()
                    .ok_or_else(|| Error::operation("allocate history compression context"))?;
                context
                    .set_parameter(CParameter::CompressionLevel(1))
                    .map_err(failure)?;
                context
                    .set_parameter(CParameter::WindowLog(WINDOW_LOG))
                    .map_err(failure)?;
                context
                    .set_parameter(CParameter::ContentSizeFlag(true))
                    .map_err(failure)?;
                slot.insert(context)
            }
        };
        context
            .reset(ResetDirective::SessionOnly)
            .map_err(failure)?;
        context.load_dictionary(dictionary(base)).map_err(failure)?;
        let mut encoded = Vec::with_capacity(zstd_safe::compress_bound(input.len()));
        context.compress2(&mut encoded, input).map_err(failure)?;
        Ok(encoded)
    }
}

#[derive(Default)]
pub(in crate::infrastructure::history) struct Decoder {
    context: Option<DCtx<'static>>,
}

impl Decoder {
    pub fn decode(
        &mut self,
        payload: &[u8],
        codec: u8,
        size: usize,
        base: &[u8],
    ) -> Result<Vec<u8>> {
        if size == 0
            || size > MAX
            || payload.len() > MAX
            || base.len() > MAX
            || codec > 2
            || (codec == 2) != !base.is_empty()
        {
            return Err(invalid());
        }
        if codec == 0 {
            return if payload.len() == size {
                Ok(payload.to_vec())
            } else {
                Err(invalid())
            };
        }
        if zstd_safe::get_frame_content_size(payload).map_err(|_| invalid())? != Some(size as u64)
            || zstd_safe::find_frame_compressed_size(payload).map_err(|_| invalid())?
                != payload.len()
        {
            return Err(invalid());
        }
        // Direct-output decoding may not need a window allocation, so enforce the
        // declared frame window as well as libzstd's allocation limit. Zstd's
        // descriptor encodes 2^(10+exponent) plus eighths of that size.
        if payload.get(4).ok_or_else(invalid)? & 0x20 == 0
            && u32::from(*payload.get(5).ok_or_else(invalid)?) > (WINDOW_LOG - 10) << 3
        {
            return Err(invalid());
        }
        let context = match &mut self.context {
            Some(context) => context,
            slot @ None => {
                let mut context = DCtx::try_create()
                    .ok_or_else(|| Error::operation("allocate history decompression context"))?;
                context
                    .set_parameter(DParameter::WindowLogMax(WINDOW_LOG))
                    .map_err(failure)?;
                slot.insert(context)
            }
        };
        context
            .reset(ResetDirective::SessionOnly)
            .map_err(failure)?;
        context.load_dictionary(dictionary(base)).map_err(failure)?;
        let mut decoded = vec![0; size];
        let mut input = InBuffer::around(payload);
        let mut output = OutBuffer::around(decoded.as_mut_slice());
        loop {
            let before = (input.pos(), output.pos());
            let remaining = context
                .decompress_stream(&mut output, &mut input)
                .map_err(failure)?;
            if remaining == 0 {
                break;
            }
            if input.pos() == payload.len() || before == (input.pos(), output.pos()) {
                return Err(invalid());
            }
        }
        if input.pos() != payload.len() || output.pos() != size {
            return Err(invalid());
        }
        Ok(decoded)
    }
}

fn dictionary(base: &[u8]) -> &[u8] {
    // Source bytes are never trained dictionaries. Excluding one magic byte prevents
    // libzstd from interpreting arbitrary source content as a dictionary header.
    if base.starts_with(&[0x37, 0xa4, 0x30, 0xec]) {
        &base[1..]
    } else {
        base
    }
}

fn failure(code: usize) -> Error {
    Error::operation(format!(
        "history chunk checksum mismatch: zstd {}",
        zstd_safe::get_error_name(code)
    ))
}

fn invalid() -> Error {
    Error::operation("history chunk checksum mismatch: invalid encoding")
}
