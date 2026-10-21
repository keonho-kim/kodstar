// Purpose: 관계와 서명에 필요한 제한된 이름·타입·조건 표현 생성.
// Pattern: Simple Module.
// Usage: 선언·바인딩·사용 사실 추출이 원문 대신 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/declaration_metadata.rs, crates/kodstar-core/src/infrastructure/parse/bindings.rs, crates/kodstar-core/src/infrastructure/parse/references.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::resolve;
use crate::domain::resolve::Reference;
use crate::infrastructure::parse::{Extractor, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn reference_text(&self, node: Node<'_>) -> Result<String> {
        if self.original_source.is_some_and(|source| {
            crate::infrastructure::parse::validation::contains_rendering(source, node.byte_range())
        }) {
            return Ok("<dynamic>()".into());
        }
        let value = text(node, self.source).trim();
        if matches!(self.language, Language::CommonLisp | Language::Kotlin) {
            return if matches!(resolve::reference(self.language, value), Reference::Dynamic) {
                Ok("<dynamic>()".into())
            } else {
                self.bounded_text(value)
            };
        }
        let plain = if self.language == Language::Rust && value.contains("r#") {
            Some(self.language.lookup_key(value))
        } else {
            None
        };
        if !name_characters(plain.as_deref().unwrap_or(value))
            || matches!(resolve::reference(self.language, value), Reference::Dynamic)
        {
            return Ok("<dynamic>()".into());
        }
        self.bounded_text(value)
    }

    pub(super) fn type_text(&self, node: Node<'_>) -> Result<String> {
        if self.original_source.is_some_and(|source| {
            crate::infrastructure::parse::validation::contains_rendering(source, node.byte_range())
        }) {
            return Ok("<dynamic-type>".into());
        }
        let value = text(node, self.source).trim_start_matches(':').trim();
        if self.language == Language::Kotlin
            && value.contains('`')
            && !matches!(resolve::reference(self.language, value), Reference::Dynamic)
        {
            return self.bounded_text(value);
        }
        // Anonymous type bodies, literal types and default values are not needed for target lookup.
        // Keep a structural marker rather than persisting their fields, tags or literal contents.
        if value.chars().all(|character| {
            character.is_alphanumeric()
                || character.is_whitespace()
                || "_.$:\\<>?,&*[]()|+-/!".contains(character)
                || character == '\'' && self.language == Language::Rust
        }) {
            self.bounded_text(value)
        } else {
            Ok("<structural-type>".into())
        }
    }

    pub(super) fn parameter_text(&self, node: Node<'_>) -> Result<String> {
        let value = text(node, self.source);
        if name_characters(value) {
            self.bounded_text(value)
        } else {
            Ok("<pattern>".into())
        }
    }
}

fn name_characters(value: &str) -> bool {
    value.chars().all(|character| {
        character.is_alphanumeric()
            || character.is_whitespace()
            || "_.$:\\?-><,&*!/+".contains(character)
    })
}

pub(super) fn conditional_attribute(value: &str) -> Option<String> {
    if value.starts_with("#[cfg(") {
        return Some(value.into());
    }
    let predicate = value.strip_prefix("#[cfg_attr(")?;
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for (index, character) in predicate.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
        } else {
            match character {
                '"' => quoted = true,
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    return Some(format!("#[cfg_attr({}, …)]", &predicate[..index]));
                }
                _ => {}
            }
        }
    }
    None
}
