// Purpose: Go manifest 행의 주석·인용 문자열·구분자를 보존해 token화.
// Pattern: Simple Module.
// Usage: Go manifest adapter가 한 행씩 호출.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/go.rs.

use crate::domain::diagnostics::{Error, Result};
use std::iter::Peekable;
use std::str::Chars;

pub(super) fn tokens(line: &str) -> Result<Vec<String>> {
    let mut chars = line.chars().peekable();
    let mut result = Vec::new();
    let mut word = String::new();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if c.is_whitespace() || matches!(c, '(' | ')') || c == '=' && chars.peek() == Some(&'>') {
            if !word.is_empty() {
                result.push(std::mem::take(&mut word));
            }
            if matches!(c, '(' | ')') {
                result.push(c.to_string());
            } else if c == '=' {
                chars.next();
                result.push("=>".into());
            }
        } else if c == '"' || c == '`' {
            if !word.is_empty() {
                return Err(Error::operation(
                    "Go manifest string must start at a token boundary",
                ));
            }
            let mut closed = false;
            quoted_value(&mut chars, c, &mut word, &mut closed)?;
            if !closed {
                return Err(Error::operation("unterminated Go manifest string"));
            }
            result.push(std::mem::take(&mut word));
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        result.push(word);
    }
    Ok(result)
}

fn quoted_value(
    chars: &mut Peekable<Chars<'_>>,
    quote: char,
    value: &mut String,
    closed: &mut bool,
) -> Result<()> {
    let mut bytes = Vec::new();
    while let Some(c) = chars.next() {
        if c == quote {
            *closed = true;
            value.push_str(
                std::str::from_utf8(&bytes)
                    .map_err(|e| Error::caused_by("non-UTF-8 Go manifest path", e))?,
            );
            return Ok(());
        }
        if c != '\\' || quote == '`' {
            bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            continue;
        }
        let escape = chars
            .next()
            .ok_or_else(|| Error::operation("incomplete Go string escape"))?;
        if escape == 'x' || ('0'..='7').contains(&escape) {
            let byte = if escape == 'x' {
                digits(chars, 16, 2, 0)?
            } else {
                digits(chars, 8, 2, escape.to_digit(8).unwrap_or(0))?
            };
            bytes.push(
                u8::try_from(byte).map_err(|_| Error::operation("Go byte escape exceeds 255"))?,
            );
            continue;
        }
        let escaped = match escape {
            'a' => '\x07',
            'b' => '\x08',
            'f' => '\x0c',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\x0b',
            '\\' => '\\',
            '"' => '"',
            'u' | 'U' => char::from_u32(digits(chars, 16, if escape == 'u' { 4 } else { 8 }, 0)?)
                .ok_or_else(|| Error::operation("invalid Go Unicode scalar escape"))?,
            _ => return Err(Error::operation("unsupported Go string escape")),
        };
        bytes.extend_from_slice(escaped.encode_utf8(&mut [0; 4]).as_bytes());
    }
    Ok(())
}

fn digits(
    chars: &mut Peekable<Chars<'_>>,
    radix: u32,
    count: usize,
    mut value: u32,
) -> Result<u32> {
    for _ in 0..count {
        let digit = chars
            .next()
            .and_then(|c| c.to_digit(radix))
            .ok_or_else(|| Error::operation("invalid Go string escape digits"))?;
        value = value
            .checked_mul(radix)
            .and_then(|n| n.checked_add(digit))
            .ok_or_else(|| Error::operation("Go string escape overflow"))?;
    }
    Ok(value)
}
