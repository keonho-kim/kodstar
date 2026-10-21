// Purpose: 기본 Lisp reader의 ASCII 대소문자·이스케이프 이름 키와 token 분류.
// Pattern: Simple Module.
// Usage: 이름 저장·조회가 같은 reader key를 사용.
// Related: crates/kodstar-core/src/domain/languages.rs, crates/kodstar-core/src/infrastructure/parse/lisp/mod.rs.

use super::Reference;

pub(crate) fn key(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let (mut quoted, mut escaped) = (false, false);
    for c in raw.chars() {
        if escaped {
            result.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '|' {
            quoted = !quoted;
        } else {
            result.push(if quoted { c } else { c.to_ascii_uppercase() });
        }
    }
    if escaped {
        result.push('\\');
    }
    result
}

pub(super) fn reference(raw: &str) -> Reference {
    let (mut quoted, mut escaped) = (false, false);
    let mut separator = None;
    for (offset, c) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == '|' {
            quoted = !quoted;
            continue;
        }
        if !quoted && c == ':' {
            if separator.is_none() {
                separator = Some(offset);
            }
            continue;
        }
        if !quoted && (c.is_whitespace() || "()'`,;\"".contains(c)) {
            return Reference::Dynamic;
        }
    }
    if raw.is_empty() || quoted || escaped {
        Reference::Dynamic
    } else if let Some(offset) = separator {
        let tail = &raw[offset + 1..];
        let (external_only, symbol) = tail.strip_prefix(':').map_or((true, tail), |s| (false, s));
        if offset == 0 || symbol.is_empty() || has_separator(symbol) {
            return Reference::Dynamic;
        }
        Reference::LispPackage {
            package: key(&raw[..offset]),
            symbol: symbol.into(),
            external_only,
        }
    } else {
        Reference::Bare(raw.into())
    }
}

fn has_separator(raw: &str) -> bool {
    let (mut quoted, mut escaped) = (false, false);
    for c in raw.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '|' {
            quoted = !quoted;
        } else if c == ':' && !quoted {
            return true;
        }
    }
    false
}
