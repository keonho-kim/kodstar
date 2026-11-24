// Purpose: 공유 헤더 본문의 주석·문자열을 제외한 C++·Objective-C 언어 근거 판별.
// Pattern: Lexical Classifier.
// Usage: 언어 판별기가 제한된 원문 버퍼를 전달하며 추가 I/O·복사 없이 검사.
// Related: crates/kodstar-core/src/infrastructure/languages/mod.rs, crates/kodstar-core/src/application/analysis/discovery.rs.

use crate::domain::languages::Language;

pub(super) fn detect(source: &[u8]) -> Option<Language> {
    let mut cpp = false;
    let mut at = 0;
    while at < source.len() {
        let rest = &source[at..];
        if rest.starts_with(b"//") {
            at += memchr::memchr(b'\n', rest).unwrap_or(rest.len());
        } else if rest.starts_with(b"/*") {
            at += memchr::memmem::find(&rest[2..], b"*/").map_or(rest.len(), |end| end + 4);
        } else if rest.starts_with(b"@interface")
            || rest.starts_with(b"@implementation")
            || rest
                .strip_prefix(b"#import")
                .is_some_and(|tail| tail.trim_ascii_start().starts_with(b"<Foundation"))
        {
            return None;
        } else if matches!(source[at], b'\'' | b'"') {
            let quote = source[at];
            at += 1;
            while at < source.len() {
                if source[at] == quote {
                    at += 1;
                    break;
                }
                at = (at + if source[at] == b'\\' { 2 } else { 1 }).min(source.len());
            }
        } else if source[at].is_ascii_alphabetic() || source[at] == b'_' {
            let start = at;
            at += 1;
            while source
                .get(at)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                at += 1;
            }
            let word = &source[start..at];
            let tail = source[at..].trim_ascii_start();
            cpp |= matches!(word, b"namespace" | b"class")
                || word == b"template" && tail.starts_with(b"<")
                || word == b"std" && tail.starts_with(b"::");
            if matches!(word, b"R" | b"u8R" | b"uR" | b"UR" | b"LR")
                && source.get(at) == Some(&b'"')
            {
                cpp = true;
                at = raw_end(source, at);
            }
        } else {
            at += 1;
        }
    }
    Some(if cpp { Language::Cpp } else { Language::C })
}

fn raw_end(source: &[u8], quote: usize) -> usize {
    let delimiter = quote + 1;
    let Some(width) = memchr::memchr(b'(', &source[delimiter..]).filter(|width| *width <= 16)
    else {
        return source.len();
    };
    let mut ending = [0; 18];
    ending[0] = b')';
    ending[1..width + 1].copy_from_slice(&source[delimiter..delimiter + width]);
    ending[width + 1] = b'"';
    let body = delimiter + width + 1;
    memchr::memmem::find(&source[body..], &ending[..width + 2])
        .map_or(source.len(), |offset| body + offset + width + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_literals_do_not_determine_the_header_language() {
        assert_eq!(
            detect(b"/* class Fake {}; @interface Fake */\nconst char* text = \"std::value\";"),
            Some(Language::C)
        );
        assert_eq!(
            detect(b"// namespace fake\nstruct Real {};"),
            Some(Language::C)
        );
        assert_eq!(
            detect(b"const char* text=R\"tag(@interface Fake \"quotes\")tag\";"),
            Some(Language::Cpp)
        );
        assert_eq!(detect(b"namespace real {}\n@interface Actual"), None);
    }
}
