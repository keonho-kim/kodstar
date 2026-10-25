// Purpose: 스트리밍 분할의 읽기 크기 독립성·삽입 재사용·취소 검증.
// Pattern: Contract Test.
// Usage: 고정 seed의 입력을 짧은 읽기와 중단 가능한 입력으로 전달.
// Related: crates/kodstar-core/src/infrastructure/history/content/chunking.rs.

use super::*;
use std::io::Cursor;

struct Short<R> {
    input: R,
    interrupted: bool,
}
impl<R: Read> Read for Short<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let length = bytes.len().min(17);
        self.input.read(&mut bytes[..length])
    }
}

fn split(input: impl Read) -> Result<Vec<Vec<u8>>> {
    let mut result = Vec::new();
    visit(input, &Cancellation::default(), |bytes| {
        result.push(bytes.to_vec());
        Ok(())
    })?;
    Ok(result)
}

#[test]
fn boundaries_ignore_short_reads_and_reuse_content_after_insertions() -> Result<()> {
    let mut state = 123456u64;
    let bytes: Vec<u8> = (0..1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let original = split(Cursor::new(&bytes))?;
    assert_eq!(
        original,
        split(Short {
            input: Cursor::new(&bytes),
            interrupted: false
        })?
    );
    assert_eq!(original.concat(), bytes);
    assert!(
        original
            .iter()
            .all(|chunk| !chunk.is_empty() && chunk.len() <= MAX)
    );
    let changed = split(Cursor::new([b"insert".as_slice(), &bytes].concat()))?;
    let reused: usize = changed
        .iter()
        .filter(|chunk| original.contains(chunk))
        .map(Vec::len)
        .sum();
    assert!(reused > bytes.len() * 9 / 10);
    Ok(())
}

#[test]
fn cancellation_and_read_errors_stop_consumption() -> Result<()> {
    let cancel = Cancellation::default();
    let mut chunks = 0;
    let result = visit(Cursor::new(vec![b'a'; MAX * 4]), &cancel, |_| {
        chunks += 1;
        cancel.cancel();
        Ok(())
    });
    assert!(matches!(
        result.expect_err("cancelled stream continued").kind,
        crate::domain::diagnostics::ErrorKind::Cancelled
    ));
    assert_eq!(chunks, 1);
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }
    assert!(visit(Broken, &Cancellation::default(), |_| Ok(())).is_err());
    Ok(())
}
