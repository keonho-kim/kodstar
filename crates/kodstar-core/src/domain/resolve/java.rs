// Purpose: Java 호출 사실의 인자 수와 표시용 표현을 함께 보존.
// Pattern: Simple Module.
// Usage: Java 추출기가 기록하고 해석기와 조회 출력이 해석.
// Related: crates/kodstar-core/src/infrastructure/parse/java.rs, crates/kodstar-core/src/application/analysis/resolve/java.rs.

use std::borrow::Cow;

pub(crate) const CALL: &str = "@java-call:";
pub(crate) const PARAMETERS: &str = "java_parameters";

pub(crate) fn call(expression: &str) -> Option<(usize, &str)> {
    let (count, spelling) = expression.strip_prefix(CALL)?.split_once('#')?;
    Some((count.parse().ok()?, spelling))
}

pub(crate) fn display(expression: &str) -> Cow<'_, str> {
    let Some((_, mut remaining)) = call(expression) else {
        return Cow::Borrowed(expression);
    };
    let mut display = String::new();
    while let Some(producer) = remaining.strip_prefix("@result:") {
        display.push_str("@result:");
        remaining = call(producer).map_or(producer, |(_, spelling)| spelling);
    }
    if display.is_empty() {
        Cow::Borrowed(remaining)
    } else {
        display.push_str(remaining);
        Cow::Owned(display)
    }
}
