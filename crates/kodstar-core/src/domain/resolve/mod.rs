// Purpose: 언어별 참조 문자열의 bare·qualified·dynamic 분류.
// Pattern: Simple Module.
// Usage: 저장 기반 이름 해석에서 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs.

//! Reference spelling is preserved; this only separates static qualification from dynamic syntax.

mod kotlin;
pub(crate) mod lisp;
pub(crate) mod namespaces;
pub(crate) mod path_patterns;

use super::languages::Language;

#[derive(Debug)]
pub(crate) enum Reference {
    Bare(String),
    LispPackage {
        package: String,
        symbol: String,
        external_only: bool,
    },
    Qualified {
        receiver: String,
        member: String,
    },
    Dynamic,
}

pub(crate) fn reference(language: Language, expression: &str) -> Reference {
    if language == Language::Kotlin {
        return kotlin::reference(expression);
    }
    if language == Language::CommonLisp {
        return lisp::reference(expression);
    }
    let mut normalized = String::with_capacity(expression.len());
    let mut generic_depth = 0usize;
    for character in expression.chars() {
        if character == '<' && !expression.starts_with("operator") {
            generic_depth += 1;
        } else if character == '>' && generic_depth > 0 {
            generic_depth -= 1;
        } else if generic_depth == 0 && !character.is_whitespace() {
            normalized.push(character);
        }
    }
    if normalized.contains(['[', ']', '(', ')', '{', '}']) && !normalized.starts_with("operator") {
        return Reference::Dynamic;
    }
    let normalized = normalized.replace("?->", "::").replace("->", "::");
    let normalized = if language == Language::Scala {
        normalized.replace('#', "::")
    } else {
        normalized
    };
    let normalized = if language == Language::Php {
        normalized.replace('\\', "::")
    } else if language == Language::Erlang {
        normalized.replace(':', "::")
    } else {
        normalized
    };
    let normalized = if matches!(
        language,
        Language::Swift | Language::Kotlin | Language::CSharp
    ) {
        normalized.replace("?.", ".").replace("!.", ".")
    } else {
        normalized
    };
    let normalized = normalized.trim_end_matches("::");
    let split = normalized
        .rfind("::")
        .map(|index| (index, 2))
        .into_iter()
        .chain(normalized.rfind('.').map(|index| (index, 1)))
        .max_by_key(|(index, _)| *index);
    match split {
        Some((index, width)) if index > 0 && index + width < normalized.len() => {
            Reference::Qualified {
                receiver: normalized[..index].into(),
                member: normalized[index + width..].into(),
            }
        }
        _ if !normalized.is_empty() => Reference::Bare(normalized.into()),
        _ => Reference::Dynamic,
    }
}
