// Purpose: 식별자·한정 이름·경로의 순위 검색 단어 경계를 정의.
// Pattern: Simple Module.
// Usage: 순위 검색 색인과 조회가 동일한 토큰 규칙을 사용.
// Related: crates/kodstar-core/src/application/query/ranked.rs, crates/kodstar-core/src/infrastructure/index/ranked_terms.rs.

pub(crate) fn words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut chars = value.chars().peekable();
    let mut previous = None::<char>;
    while let Some(current) = chars.next() {
        if !current.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            previous = None;
            continue;
        }
        if current.is_uppercase()
            && previous.is_some_and(|p| {
                p.is_lowercase()
                    || (p.is_uppercase() && chars.peek().is_some_and(|c| c.is_lowercase()))
            })
        {
            words.push(std::mem::take(&mut word));
        }
        word.extend(current.to_lowercase());
        previous = Some(current);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}
