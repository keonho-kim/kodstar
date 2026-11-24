// Purpose: 원문 바이트 위치를 제한된 줄 색인으로 정확한 줄·byte column에 대응.
// Pattern: Immutable Index.
// Usage: 명시적 인코딩과 증분 삽입 증명이 반복 span 복원에 사용.
// Related: crates/kodstar-core/src/domain/source.rs, crates/kodstar-core/src/domain/facts/append/source.rs.

use super::SourceSpan;

pub(crate) struct Lines {
    starts: Vec<usize>,
    length: usize,
}
impl Lines {
    pub fn new(source: &[u8], budget: usize) -> Option<Self> {
        Self::joined(source, &[], budget)
    }
    pub fn joined(prefix: &[u8], suffix: &[u8], budget: usize) -> Option<Self> {
        let count = memchr::memchr_iter(b'\n', prefix)
            .count()
            .checked_add(memchr::memchr_iter(b'\n', suffix).count())?
            .checked_add(1)?;
        if count.checked_mul(std::mem::size_of::<usize>())? > budget {
            return None;
        }
        let mut starts = Vec::with_capacity(count);
        starts.push(0);
        starts.extend(memchr::memchr_iter(b'\n', prefix).map(|p| p + 1));
        starts.extend(memchr::memchr_iter(b'\n', suffix).map(|p| prefix.len() + p + 1));
        Some(Self {
            starts,
            length: prefix.len().checked_add(suffix.len())?,
        })
    }
    pub fn owned_bytes(&self) -> usize {
        self.starts.capacity() * std::mem::size_of::<usize>()
    }
    pub fn span(&self, start: usize, end: usize) -> SourceSpan {
        let start = start.min(self.length);
        let end = end.max(start).min(self.length);
        let a = self
            .starts
            .partition_point(|&p| p <= start)
            .saturating_sub(1);
        let b = self.starts.partition_point(|&p| p <= end).saturating_sub(1);
        SourceSpan {
            start_byte: start,
            end_byte: end,
            line: a + 1,
            column: start - self.starts[a] + 1,
            end_line: b + 1,
            end_column: end - self.starts[b] + 1,
        }
    }
}
