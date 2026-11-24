// Purpose: Kotlin 다중 달러 문자열의 실제 보간 구간을 원본 바이트 위치로 유지.
// Pattern: Lexical Adapter.
// Usage: Kotlin 입력을 canonical grammar에 전달하기 전에 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/input/mod.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::borrow::Cow;

pub(super) fn prepare<'a>(source: &'a [u8], cancel: &Cancellation) -> Result<Cow<'a, [u8]>> {
    cancel.check()?;
    if memchr::memmem::find(source, b"$\"").is_none() {
        return Ok(Cow::Borrowed(source));
    }
    let mut input = Input {
        source,
        output: Cow::Borrowed(source),
        cancel,
    };
    input.code(0, false, 0)?;
    Ok(input.output)
}

struct Input<'a, 'cancel> {
    source: &'a [u8],
    output: Cow<'a, [u8]>,
    cancel: &'cancel Cancellation,
}

impl Input<'_, '_> {
    fn depth(&self, at: usize, depth: usize) -> Result<()> {
        if at.is_multiple_of(4096) {
            self.cancel.check()?;
        }
        if depth > 1024 {
            return Err(Error::new(
                ErrorKind::Limit,
                "Kotlin string nesting exceeds 1024 levels",
            ));
        }
        Ok(())
    }

    fn code(&mut self, mut at: usize, interpolation: bool, depth: usize) -> Result<usize> {
        let mut braces = 0;
        while at < self.source.len() {
            self.depth(at, depth)?;
            if self.source[at..].starts_with(b"//") {
                at += memchr::memchr(b'\n', &self.source[at..]).unwrap_or(self.source.len() - at);
            } else if self.source[at..].starts_with(b"/*") {
                at = self.comment(at)?;
            } else if self.source[at] == b'\'' || self.source[at] == b'`' {
                let quote = self.source[at];
                at += 1;
                while at < self.source.len() && self.source[at] != quote {
                    self.depth(at, depth)?;
                    at += if self.source[at] == b'\\' && quote == b'\'' {
                        2
                    } else {
                        1
                    };
                }
                at = (at + 1).min(self.source.len());
            } else if self.source[at] == b'"' {
                at = self.string(at, 1, false, depth + 1)?;
            } else if self.source[at] == b'$' {
                let start = at;
                while self.source.get(at) == Some(&b'$') {
                    at += 1;
                }
                if self.source.get(at) == Some(&b'"') {
                    self.output.to_mut()[start..at].fill(b' ');
                    at = self.string(at, at - start, true, depth + 1)?;
                }
            } else {
                match self.source[at] {
                    b'{' => braces += 1,
                    b'}' if interpolation && braces == 0 => return Ok(at + 1),
                    b'}' if braces > 0 => braces -= 1,
                    _ => {}
                }
                at += 1;
            }
        }
        Ok(at)
    }

    fn comment(&self, mut at: usize) -> Result<usize> {
        at += 2;
        let mut nesting = 1;
        while at < self.source.len() {
            self.depth(at, 0)?;
            if self.source[at..].starts_with(b"/*") {
                nesting += 1;
                at += 2;
            } else if self.source[at..].starts_with(b"*/") {
                nesting -= 1;
                at += 2;
                if nesting == 0 {
                    break;
                }
            } else {
                at += 1;
            }
        }
        Ok(at)
    }

    fn string(
        &mut self,
        mut at: usize,
        dollars: usize,
        rewrite: bool,
        depth: usize,
    ) -> Result<usize> {
        self.depth(at, depth)?;
        let quotes = if self.source[at..].starts_with(b"\"\"\"") {
            3
        } else {
            1
        };
        at += quotes;
        while at < self.source.len() {
            self.depth(at, depth)?;
            if self.source[at..].starts_with(&b"\"\"\""[..quotes]) {
                at += quotes;
                if quotes == 3 && self.source.get(at) == Some(&b'"') {
                    at += 1;
                }
                break;
            }
            if self.source[at] == b'\\' && quotes == 1 {
                at = (at + 2).min(self.source.len());
            } else if self.source[at] == b'$' {
                let start = at;
                while self.source.get(at) == Some(&b'$') {
                    at += 1;
                }
                let active = at - start >= dollars
                    && (self.source.get(at) == Some(&b'{') || self.identifier(at));
                if rewrite {
                    let end = if active { at - 1 } else { at };
                    self.output.to_mut()[start..end].fill(b'_');
                }
                if active && self.source.get(at) == Some(&b'{') {
                    at = self.code(at + 1, true, depth + 1)?;
                }
            } else {
                at += 1;
            }
        }
        Ok(at)
    }

    fn identifier(&self, at: usize) -> bool {
        let Some(first) = self.source.get(at) else {
            return false;
        };
        let width = match first {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return false,
        };
        self.source
            .get(at..at + width)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.chars().next())
            .is_some_and(|c| c == '_' || c.is_alphabetic())
    }
}
