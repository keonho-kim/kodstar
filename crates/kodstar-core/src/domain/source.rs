// Purpose: 원본 바이트 위치와 손실 없는 경로 표현 값.
// Pattern: Simple Module.
// Usage: 구문 사실·객체·결과가 사용.
// Related: crates/kodstar-core/src/domain/facts.rs, crates/kodstar-core/src/domain/symbols.rs.

//! Original UTF-8 byte positions. Display columns are byte columns, not character counts.

#[derive(serde::Serialize, Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SourceSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct PathEncoding {
    pub platform: &'static str,
    pub bytes_base64: String,
}

impl SourceSpan {
    pub(crate) fn at(source: &[u8], start: usize, end: usize) -> Self {
        let start = start.min(source.len());
        let end = end.max(start).min(source.len());
        let line = memchr::memchr_iter(b'\n', &source[..start]).count() + 1;
        let line_start = memchr::memrchr(b'\n', &source[..start]).map_or(0, |offset| offset + 1);
        let end_line = line + memchr::memchr_iter(b'\n', &source[start..end]).count();
        let end_line_start = memchr::memrchr(b'\n', &source[..end]).map_or(0, |offset| offset + 1);
        Self {
            start_byte: start,
            end_byte: end,
            line,
            column: start - line_start + 1,
            end_line,
            end_column: end - end_line_start + 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_columns_and_crlf_preserve_original_bytes() {
        let source = "한글\r\n  target()".as_bytes();
        let span = SourceSpan::at(source, 10, 16);
        assert_eq!((span.line, span.column), (2, 3));
        assert_eq!(&source[span.start_byte..span.end_byte], b"target");
    }
}
