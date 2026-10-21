// Purpose: 분석 입력 종류에 맞는 code·component·설정 adapter 선택.
// Pattern: Simple Module.
// Usage: worker와 설정 확장 단계가 제한된 입력을 전달.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/infrastructure/formats/mod.rs.

use super::events::InputKind;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::facts::Fact;
use crate::domain::languages::Language;
use crate::infrastructure::formats;
use crate::infrastructure::parse::ParseSession;
use std::path::Path;

pub(super) fn run(
    parser: &mut ParseSession,
    path: &Path,
    source: &[u8],
    kind: InputKind,
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
) -> Result<()> {
    match kind {
        InputKind::Code(language) if language.is_component() => {
            formats::extract_component(parser, language, path, source, cancel, emit)
        }
        InputKind::Code(language) => parser.extract(language, path, source, cancel, &mut |fact| {
            emit(language, fact)
        }),
        InputKind::Auxiliary => formats::extract_auxiliary(parser, path, source, cancel, emit),
        InputKind::Configuration => {
            formats::extract_configuration(parser, path, source, cancel, emit)
        }
    }
}
