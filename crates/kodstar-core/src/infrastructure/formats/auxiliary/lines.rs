// Purpose: 행 기반 보조 입력의 원본 바이트 위치를 한 번의 순회로 계산.
// Pattern: Streaming Iterator.
// Usage: text·go·pnpm adapter가 UTF-8 검증 후 행과 위치를 소비.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/text.rs, crates/kodstar-core/src/infrastructure/formats/auxiliary/go.rs, crates/kodstar-core/src/infrastructure/formats/auxiliary/pnpm.rs.

use crate::domain::source::SourceSpan;

pub(super) fn lines(source: &str) -> impl Iterator<Item = (&str, SourceSpan)> {
    source
        .split_inclusive('\n')
        .enumerate()
        .scan(0, |offset, (index, line)| {
            let start = *offset;
            let length = line.trim_end().len();
            *offset += line.len();
            Some((
                line,
                SourceSpan {
                    start_byte: start,
                    end_byte: start + length,
                    line: index + 1,
                    column: 1,
                    end_line: index + 1,
                    end_column: length + 1,
                },
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unicode_crlf_empty_lines_and_trailing_whitespace_positions() {
        for source in [
            "",
            "\n",
            "한글\r\n\n  OnClick = 실행  \r\n끝\t",
            "first\nlast",
        ] {
            let mut offset = 0;
            for (line, location) in lines(source) {
                assert_eq!(
                    location,
                    SourceSpan::at(source.as_bytes(), offset, offset + line.trim_end().len())
                );
                offset += line.len();
            }
            assert_eq!(offset, source.len());
        }
    }
}
