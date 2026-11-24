// Purpose: 실행 가능한 script 본문을 한 번에 하나씩 판별하고 style·비코드 입력 제외.
// Pattern: Streaming Scanner.
// Usage: component mask와 EJS adapter가 ScriptCursor를 전진시킴.
// Related: crates/kodstar-core/src/infrastructure/formats/components/regions.rs, crates/kodstar-core/src/infrastructure/formats/components/ejs/mod.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::languages::Language;
use crate::infrastructure::formats::components::regions::{find_ascii_case, next_markup, tag_end};
use std::ops::Range;

pub(super) struct Script {
    pub body: Range<usize>,
    pub language: Language,
    pub jsx: bool,
    pub module: bool,
    pub attributes: bool,
}
pub(super) struct ScriptCursor {
    offset: usize,
    format: Language,
    initial: Language,
}
impl ScriptCursor {
    pub fn new(format: Language, initial: Language) -> Self {
        Self {
            offset: 0,
            format,
            initial,
        }
    }
    pub fn next(
        &mut self,
        source: &[u8],
        code: &[u8],
        cancel: &Cancellation,
    ) -> Result<Option<Script>> {
        while let Some(start) = next_markup(source, &mut self.offset, self.format, code, cancel)? {
            if !source.get(start + 1).is_some_and(u8::is_ascii_alphabetic) {
                continue;
            }
            let name_end = source[start + 1..]
                .iter()
                .position(|b| !b.is_ascii_alphanumeric() && *b != b'-')
                .map_or(source.len(), |i| start + 1 + i);
            let name = &source[start + 1..name_end];
            let opening = tag_end(source, name_end)?;
            self.offset = opening + 1;
            if !name.eq_ignore_ascii_case(b"script") && !name.eq_ignore_ascii_case(b"style") {
                continue;
            }
            let delimiter = if name.eq_ignore_ascii_case(b"script") {
                b"</script".as_slice()
            } else {
                b"</style".as_slice()
            };
            let closing = closing(source, code, self.offset, delimiter, cancel)?;
            let mut script = Script {
                body: self.offset..closing,
                language: self.initial,
                jsx: false,
                module: false,
                attributes: false,
            };
            self.offset = tag_end(source, closing + delimiter.len())? + 1;
            if name.eq_ignore_ascii_case(b"style") {
                continue;
            }
            let mut executable = true;
            crate::infrastructure::formats::markup::attributes(
                source,
                name_end..opening,
                &mut |attribute| {
                    script.attributes |= !attribute.name.eq_ignore_ascii_case("src");
                    if attribute.name.eq_ignore_ascii_case("lang") {
                        if matches!(attribute.value, "ts" | "tsx") {
                            script.language = Language::TypeScript;
                        }
                        script.jsx = matches!(attribute.value, "jsx" | "tsx");
                    }
                    if attribute.name.eq_ignore_ascii_case("type") {
                        script.module = attribute.value.eq_ignore_ascii_case("module");
                        executable &= attribute.value.is_empty()
                            || script.module
                            || [
                                "text/javascript",
                                "application/javascript",
                                "text/ecmascript",
                                "application/ecmascript",
                                "text/typescript",
                                "application/typescript",
                            ]
                            .iter()
                            .any(|s| attribute.value.eq_ignore_ascii_case(s));
                    }
                    if attribute.name.eq_ignore_ascii_case("src") {
                        executable = false;
                    }
                    Ok(())
                },
            )?;
            if executable {
                return Ok(Some(script));
            }
        }
        Ok(None)
    }
}

fn closing(
    source: &[u8],
    code: &[u8],
    mut at: usize,
    delimiter: &[u8],
    cancel: &Cancellation,
) -> Result<usize> {
    while let Some(start) = find_ascii_case(source, at, delimiter) {
        cancel.check()?;
        let after = start + delimiter.len();
        if code.get(start) != Some(&1)
            && source
                .get(after)
                .is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'>' | b'/'))
        {
            return Ok(start);
        }
        at = after;
    }
    Err(Error::input(
        "source_syntax_error",
        "component script/style has no closing tag",
        None,
    ))
}

pub(super) struct Scripts {
    pub language: Language,
    pub jsx: bool,
    pub parts: usize,
}
pub(super) fn copy_scripts(
    format: Language,
    source: &[u8],
    mask: &mut [u8],
    initial: Language,
    cancel: &Cancellation,
) -> Result<Scripts> {
    let mut result = Scripts {
        language: initial,
        jsx: false,
        parts: 0,
    };
    let mut cursor = ScriptCursor::new(format, initial);
    while let Some(script) = cursor.next(source, mask, cancel)? {
        if script.language == Language::TypeScript {
            result.language = script.language;
        }
        result.jsx |= script.jsx;
        mask[script.body.clone()].copy_from_slice(&source[script.body]);
        result.parts += 1;
    }
    Ok(result)
}
