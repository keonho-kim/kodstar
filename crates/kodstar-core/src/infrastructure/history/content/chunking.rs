// Purpose: FastCDC로 큰 원문을 제한된 청크로 나누고 취소·I/O 실패를 전달.
// Pattern: Streaming Adapter.
// Usage: blob writer가 한 청크를 저장한 뒤 다음 청크를 요청.
// Related: crates/kodstar-core/src/infrastructure/history/content/blobs.rs, crates/kodstar-core/src/infrastructure/history/content/chunking/tests.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use std::io::{self, Read};

const MIN: usize = 2048;
const AVERAGE: usize = 8192;
const MAX: usize = 32768;

#[cfg(test)]
mod tests;

pub(in crate::infrastructure::history) fn visit(
    input: impl Read,
    cancel: &Cancellation,
    mut consume: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    cancel.check()?;
    let source = Source { input, cancel };
    let chunks = fastcdc::v2020::StreamCDC::new(source, MIN, AVERAGE, MAX);
    for chunk in chunks {
        cancel.check()?;
        let chunk = chunk.map_err(|e| Error::caused_by("read history chunks", e))?;
        consume(&chunk.data)?;
    }
    cancel.check()
}

struct Source<'a, R> {
    input: R,
    cancel: &'a Cancellation,
}

impl<R: Read> Read for Source<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            self.cancel.check().map_err(io::Error::other)?;
            match self.input.read(bytes) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    }
}
