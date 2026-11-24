// Purpose: 언어가 정의한 입력 표현을 원본 길이·줄 경계를 보존해 준비.
// Pattern: Source Adapter.
// Usage: parser 진입점과 session이 문법 호환 입력을 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/infrastructure/parse/input/kotlin.rs.

pub(super) mod encoding;
mod erlang;
mod kotlin;
pub(super) mod native;
mod native_definitions;
mod native_evidence;
mod native_headers;
mod native_ranges;
mod native_session;
mod native_tokens;
mod native_types;
pub(super) mod native_validation;
pub(super) mod typescript;
pub(super) use native_headers::has_macro_functions;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use std::borrow::Cow;

pub(super) fn syntax<'a>(
    language: Language,
    source: &'a [u8],
    cancel: &Cancellation,
) -> Result<Cow<'a, [u8]>> {
    match language {
        Language::Kotlin => kotlin::prepare(source, cancel),
        _ => Ok(Cow::Borrowed(source)),
    }
}
