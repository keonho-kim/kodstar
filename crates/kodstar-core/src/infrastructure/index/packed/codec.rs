// Purpose: 압축 인덱스 블록의 정규 가변 정수·문자열 경계.
// Pattern: Bounded Encoding.
// Usage: dictionary와 relations codec이 공유하는 바이트 계약.
// Related: crates/kodstar-core/src/infrastructure/index/packed/mod.rs.

use rusqlite::{Error, Result};

pub(crate) fn invalid() -> Error {
    Error::ModuleError("invalid packed index block".into())
}

pub(crate) fn integer(bytes: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        bytes.push(value as u8 | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
}

pub(crate) fn text(bytes: &mut Vec<u8>, value: &str) {
    integer(bytes, value.len() as u64);
    bytes.extend_from_slice(value.as_bytes());
}

pub(crate) struct Input<'a>(pub &'a [u8]);

impl<'a> Input<'a> {
    pub fn integer(&mut self) -> Result<u64> {
        let mut value = 0;
        for shift in (0..=63).step_by(7) {
            let (&byte, rest) = self.0.split_first().ok_or_else(invalid)?;
            self.0 = rest;
            if shift == 63 && byte > 1 || shift > 0 && byte == 0 {
                return Err(invalid());
            }
            value |= u64::from(byte & 127) << shift;
            if byte < 128 {
                return Ok(value);
            }
        }
        Err(invalid())
    }

    pub fn id(&mut self) -> Result<i64> {
        i64::try_from(self.integer()?).map_err(|_| invalid())
    }

    pub fn text_ref(&mut self) -> Result<&'a str> {
        let length = usize::try_from(self.integer()?).map_err(|_| invalid())?;
        let bytes = self.0.get(..length).ok_or_else(invalid)?;
        let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
        self.0 = &self.0[length..];
        Ok(text)
    }

    pub fn finish(self) -> Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
