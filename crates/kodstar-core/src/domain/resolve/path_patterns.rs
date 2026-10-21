// Purpose: Match exact, wildcard and directory-prefix module path patterns.
// Pattern: Simple Module.
// Usage: JavaScript configuration and package path resolution.
// Related: crates/kodstar-core/src/application/analysis/resolve/javascript/mod.rs, crates/kodstar-core/src/application/analysis/resolve/javascript/targets.rs.

pub(crate) struct PatternMatch<'a> {
    pub(crate) rank: u8,
    pub(crate) prefix: usize,
    pub(crate) captured: &'a str,
    pub(crate) wildcard: bool,
}

pub(crate) fn match_pattern<'a>(raw: &'a str, pattern: &str) -> Option<PatternMatch<'a>> {
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        if suffix.contains('*')
            || !raw.starts_with(prefix)
            || !raw.ends_with(suffix)
            || raw.len() < prefix.len() + suffix.len()
        {
            return None;
        }
        return Some(PatternMatch {
            rank: 1,
            prefix: prefix.len(),
            captured: &raw[prefix.len()..raw.len() - suffix.len()],
            wildcard: true,
        });
    }
    if raw == pattern {
        return Some(PatternMatch {
            rank: 0,
            prefix: pattern.len(),
            captured: "",
            wildcard: false,
        });
    }
    let prefix = pattern.trim_end_matches('/');
    if prefix.is_empty() {
        return None;
    }
    raw.strip_prefix(prefix)
        .and_then(|tail| tail.strip_prefix('/'))
        .map(|captured| PatternMatch {
            rank: 2,
            prefix: prefix.len(),
            captured,
            wildcard: false,
        })
}
