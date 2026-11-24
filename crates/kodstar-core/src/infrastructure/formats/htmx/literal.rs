// Purpose: 정적 HTML 문자열의 htmx 사실을 원래 JS/TS 위치와 함수 소유자로 환산.
// Pattern: Embedded Range Adapter.
// Usage: Hono 응답 추출기가 실행 없는 문자열 구간을 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/references.rs, crates/kodstar-core/src/infrastructure/parse/javascript/hono/responses.rs.

use super::Owner;
use crate::domain::{
    cancellation::Cancellation, diagnostics::Result, facts::Fact, languages::Language,
    source::SourceSpan,
};

pub(in crate::infrastructure) fn extract(
    source: &[u8],
    origin: SourceSpan,
    owner: Owner,
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    let mask = vec![0; source.len()];
    crate::infrastructure::formats::components::references::template_references(
        Language::Html,
        source,
        owner.language,
        &mask,
        cancel,
        &mut |_, mut fact| {
            let span = match &mut fact {
                Fact::Use(u) => {
                    u.source = owner.source;
                    u.scope = owner.scope;
                    &mut u.span
                }
                Fact::Binding(b) => {
                    b.scope = owner.scope;
                    &mut b.span
                }
                _ => return Ok(()),
            };
            if span.line == 1 {
                span.column += origin.column - 1;
            }
            if span.end_line == 1 {
                span.end_column += origin.column - 1;
            }
            span.line += origin.line - 1;
            span.end_line += origin.line - 1;
            span.start_byte += origin.start_byte;
            span.end_byte += origin.start_byte;
            emit(fact)
        },
    )
}
