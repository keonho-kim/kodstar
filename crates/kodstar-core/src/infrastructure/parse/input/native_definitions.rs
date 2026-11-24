// Purpose: 오류 복구 AST와 독립적으로 전처리 지시문 범위·함수형 매크로 이름 수집.
// Pattern: Lexical Scanner.
// Usage: native evidence가 독립 호출의 보정 근거를 준비.
// Related: crates/kodstar-core/src/infrastructure/parse/input/native_evidence.rs, crates/kodstar-core/src/infrastructure/parse/input/native_tokens.rs.

use super::native_tokens::{directive_end, token_end};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::ops::Range;

pub(super) struct Definitions {
    pub names: Vec<Range<usize>>,
    pub directives: Vec<Range<usize>>,
}

pub(super) fn collect(source: &[u8], limit: usize, cancel: &Cancellation) -> Result<Definitions> {
    let mut names = Vec::new();
    let mut directives = Vec::new();
    let mut at = 0;
    while at < source.len() {
        cancel.check()?;
        if let Some(end) = directive_end(source, at) {
            directives.push(at..end);
            if (names.len() + directives.len()).saturating_mul(64) > limit {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "native directives exceeded their byte budget",
                ));
            }
            let line = source[at + 1..end].trim_ascii_start();
            if let Some(rest) = line.strip_prefix(b"define")
                && rest.first().is_some_and(u8::is_ascii_whitespace)
            {
                let name = rest.trim_ascii_start();
                let count = name
                    .iter()
                    .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                    .count();
                if count > 0 && name.get(count) == Some(&b'(') {
                    let offset = end - name.len();
                    super::native_types::remember(&mut names, offset..offset + count, source);
                    if names.len().saturating_mul(64) > limit {
                        return Err(Error::new(
                            ErrorKind::Limit,
                            "native macro definitions exceeded their byte budget",
                        ));
                    }
                }
            }
            at = end;
        } else {
            at = token_end(source, at).unwrap_or(at + 1);
        }
    }
    Ok(Definitions { names, directives })
}
