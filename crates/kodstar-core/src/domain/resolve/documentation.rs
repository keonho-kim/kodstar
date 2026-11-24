// Purpose: AST 주석에서 추출한 명시 문서 참조의 저장 표현 정의.
// Pattern: Simple Module.
// Usage: parser가 기록하고 explain 및 일반 관계 해석이 구분.
// Related: crates/kodstar-core/src/infrastructure/parse/documentation.rs, crates/kodstar-core/src/application/query/documentation.rs.

pub(crate) const PREFIX: &str = "@design-reference:";

pub(crate) fn reference(expression: &str) -> Option<(&'static str, &str)> {
    let value = expression.strip_prefix(PREFIX)?;
    let (placement, mention) = value.split_once(':')?;
    if !matches!(placement, "leading" | "inside") {
        return None;
    }
    let (kind, target) = mention.split_once(':')?;
    let kind = match kind {
        "adr" => "adr",
        "rfc" => "rfc",
        _ => return None,
    };
    Some((kind, target))
}
