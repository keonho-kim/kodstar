// Purpose: 예산 안의 XML 개행 색인과 원본 바이트 위치 계산.
// Pattern: Bounded Position Index.
// Usage: XML·.NET adapter가 요소와 속성 범위를 SourceSpan으로 변환.
// Related: crates/kodstar-core/src/infrastructure/formats/xml.rs, crates/kodstar-core/src/domain/source.rs.

use crate::domain::source::SourceSpan;
use std::ops::Range;

pub(in crate::infrastructure::formats) struct Positions<'a> {
    source: &'a [u8],
    lines: Vec<usize>,
}

impl<'a> Positions<'a> {
    pub fn new(source: &'a [u8], budget: usize) -> Self {
        let count = memchr::memchr_iter(b'\n', source).count() + 1;
        let mut lines = Vec::new();
        if count.saturating_mul(size_of::<usize>()) <= budget {
            lines.reserve_exact(count);
            lines.push(0);
            lines.extend(memchr::memchr_iter(b'\n', source).map(|offset| offset + 1));
        }
        Self { source, lines }
    }

    pub fn span(&self, range: Range<usize>) -> SourceSpan {
        if self.lines.is_empty() {
            return SourceSpan::at(self.source, range.start, range.end);
        }
        let line = self.lines.partition_point(|&offset| offset <= range.start);
        let end_line = self.lines.partition_point(|&offset| offset <= range.end);
        SourceSpan {
            start_byte: range.start,
            end_byte: range.end,
            line,
            column: range.start - self.lines[line - 1] + 1,
            end_line,
            end_column: range.end - self.lines[end_line - 1] + 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_and_direct_positions_keep_unicode_crlf_and_eof_identical() {
        let source = "한글\r\n<tag a='값'/>\n".as_bytes();
        for budget in [0, 1024] {
            let positions = Positions::new(source, budget);
            for start in 0..=source.len() {
                assert_eq!(
                    positions.span(start..source.len()),
                    SourceSpan::at(source, start, source.len())
                );
            }
        }
    }
}
