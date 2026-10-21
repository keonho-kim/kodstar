// Purpose: Kotlin escaped 식별자 안의 문자와 실제 한정 구분자 분리.
// Pattern: Simple Module.
// Usage: 공통 reference 분류기가 Kotlin 표현식에서 호출.
// Related: crates/kodstar-core/src/domain/resolve/mod.rs, crates/kodstar-core/src/domain/languages.rs.

use super::Reference;

pub(super) fn reference(expression: &str) -> Reference {
    let mut name = String::with_capacity(expression.len());
    let mut quoted = false;
    let mut generic_depth = 0usize;
    let mut split = None;
    for character in expression.chars() {
        if character == '`' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            if matches!(character, '\n' | '\r') {
                return Reference::Dynamic;
            }
            if generic_depth == 0 {
                name.push(character);
            }
            continue;
        }
        if character == '<' {
            generic_depth += 1;
            continue;
        }
        if character == '>' {
            if generic_depth == 0 {
                return Reference::Dynamic;
            }
            generic_depth -= 1;
            continue;
        }
        if generic_depth > 0 || character.is_whitespace() {
            continue;
        }
        if character == '.' {
            if name.ends_with(['?', '!']) {
                name.pop();
            }
            split = Some((name.len(), 1));
        } else if character == ':' && name.ends_with(':') {
            split = Some((name.len() - 1, 2));
        } else if !character.is_alphanumeric() && !matches!(character, '_' | '$' | '?' | '!' | ':')
        {
            return Reference::Dynamic;
        }
        name.push(character);
    }
    if quoted || generic_depth != 0 {
        return Reference::Dynamic;
    }
    match split {
        Some((at, width)) if at > 0 && at + width < name.len() => Reference::Qualified {
            receiver: name[..at].into(),
            member: name[at + width..].into(),
        },
        None if !name.is_empty() => Reference::Bare(name),
        _ => Reference::Dynamic,
    }
}
