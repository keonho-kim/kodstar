// Purpose: 명시된 소스 인코딩의 엄격한 디코딩과 원본 바이트 위치 복원.
// Pattern: Boundary Adapter.
// Usage: ParseSession이 Python·Ruby 입력의 첫 두 줄 선언을 확인.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/domain/source.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    facts::Fact,
    languages::Language,
};
use encoding_rs::{DecoderResult, Encoding, UTF_8};

pub(in crate::infrastructure::parse) struct Source {
    pub bytes: Vec<u8>,
    offsets: Vec<(usize, usize)>,
    lines: crate::domain::source::lines::Lines,
}

impl Source {
    pub fn read(
        language: Language,
        source: &[u8],
        budget: usize,
        cancel: &Cancellation,
    ) -> Result<Option<Self>> {
        if !matches!(language, Language::Python | Language::Ruby) {
            return Ok(None);
        }
        let Some(mut label) = declaration(source) else {
            return Ok(None);
        };
        if language == Language::Ruby && matches!(label.as_str(), "binary" | "ascii-8bit") {
            label = "iso-8859-1".into();
        }
        if matches!(label.as_str(), "ascii" | "us-ascii") {
            return if source.is_ascii() {
                Ok(None)
            } else {
                Err(invalid("non-ASCII bytes in ASCII source"))
            };
        }
        if !matches!(
            label.as_str(),
            "utf-8"
                | "utf8"
                | "latin-1"
                | "latin1"
                | "iso-8859-1"
                | "iso8859-1"
                | "iso-8859-5"
                | "iso8859-5"
                | "windows-1251"
                | "cp1251"
                | "windows-1252"
                | "cp1252"
                | "koi8-r"
                | "koi8-u"
        ) {
            return Err(invalid("unsupported declared source encoding"));
        }
        if matches!(label.as_str(), "windows-1252" | "cp1252")
            && source
                .iter()
                .any(|b| matches!(b, 0x81 | 0x8d | 0x8f | 0x90 | 0x9d))
            || matches!(label.as_str(), "windows-1251" | "cp1251") && source.contains(&0x98)
        {
            return Err(invalid("undefined bytes in declared source encoding"));
        }
        let latin1 = matches!(
            label.as_str(),
            "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1"
        );
        let encoding = Encoding::for_label(label.as_bytes())
            .ok_or_else(|| invalid("unsupported declared source encoding"))?;
        if encoding == UTF_8 {
            return Ok(None);
        }
        if !encoding.is_ascii_compatible() || encoding.name() == "ISO-2022-JP" {
            return Err(invalid(
                "declared source encoding must preserve ASCII syntax",
            ));
        }
        let mut decoder = encoding.new_decoder_without_bom_handling();
        let mut result = Self {
            bytes: Vec::new(),
            offsets: vec![(0, 0)],
            lines: crate::domain::source::lines::Lines::new(source, budget).ok_or_else(|| {
                Error::input(
                    "source_analysis_limit",
                    "source line index exceeds file workspace",
                    None,
                )
            })?,
        };
        for (position, &byte) in source.iter().enumerate() {
            if position.is_multiple_of(4096) {
                cancel.check()?;
            }
            let mut output = [0; 32];
            let written = if latin1 {
                char::from(byte).encode_utf8(&mut output).len()
            } else {
                let (status, read, written) = decoder.decode_to_utf8_without_replacement(
                    &source[position..position + 1],
                    &mut output,
                    position + 1 == source.len(),
                );
                if !matches!(status, DecoderResult::InputEmpty) || read != 1 {
                    return Err(invalid("malformed bytes in declared source encoding"));
                }
                written
            };
            result.bytes.extend_from_slice(&output[..written]);
            if written > 0 {
                let next = (result.bytes.len(), position + 1);
                let previous = result.offsets.last().copied().unwrap_or((0, 0));
                if next.0 - previous.0 != next.1 - previous.1 {
                    result.offsets.push(next);
                }
            }
            if result.bytes.capacity() + result.offsets.capacity() * 16 + result.lines.owned_bytes()
                > budget
            {
                return Err(Error::input(
                    "source_analysis_limit",
                    "source encoding projection exceeds file workspace",
                    None,
                ));
            }
        }
        Ok(Some(result))
    }

    pub fn restore(&self, fact: &mut Fact) {
        let span = match fact {
            Fact::Definition(value) => &mut value.span,
            Fact::Scope(value) => &mut value.span,
            Fact::Binding(value) => &mut value.span,
            Fact::Use(value) => &mut value.span,
            Fact::Module { .. } => return,
        };
        *span = self.restore_span(*span);
    }

    pub fn restore_span(
        &self,
        span: crate::domain::source::SourceSpan,
    ) -> crate::domain::source::SourceSpan {
        self.lines
            .span(self.original(span.start_byte), self.original(span.end_byte))
    }

    fn original(&self, offset: usize) -> usize {
        let i = self
            .offsets
            .partition_point(|&(decoded, _)| decoded <= offset)
            .saturating_sub(1);
        let (decoded, original) = self.offsets[i];
        original + offset - decoded
    }
}

fn declaration(source: &[u8]) -> Option<String> {
    let (begin, end) = declaration_range(source)?;
    Some(
        std::str::from_utf8(&source[begin..end])
            .ok()?
            .to_ascii_lowercase()
            .replace('_', "-"),
    )
}

fn declaration_range(source: &[u8]) -> Option<(usize, usize)> {
    let mut start = 0;
    for line in source.split_inclusive(|&b| b == b'\n').take(2) {
        let Some(comment) = line.iter().position(|b| !b.is_ascii_whitespace()) else {
            start += line.len();
            continue;
        };
        if line[comment] == b'#' {
            for keyword in [b"coding".as_slice(), b"encoding".as_slice()] {
                if let Some(at) = memchr::memmem::find(&line[comment..], keyword) {
                    let mut at = comment + at + keyword.len();
                    if matches!(line.get(at), Some(b':' | b'=')) {
                        at += 1;
                        while line.get(at).is_some_and(u8::is_ascii_whitespace) {
                            at += 1;
                        }
                        let end = at
                            + line[at..]
                                .iter()
                                .take_while(|b| {
                                    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')
                                })
                                .count();
                        return Some((start + at, start + end));
                    }
                }
            }
        }
        start += line.len();
    }
    None
}

fn invalid(message: &str) -> Error {
    Error::input("source_encoding_error", message, None)
}
