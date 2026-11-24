// Purpose: HTTP URL 경계와 정적 Hono 경로 결합·제한된 패턴 비교.
// Pattern: Functional Core.
// Usage: 웹 추출기와 HTTP resolver가 I/O 없이 호출.
// Related: crates/kodstar-core/src/domain/web/mod.rs.

pub(crate) fn request_path(raw: &str) -> Option<String> {
    if !raw.starts_with('/')
        || raw.starts_with("//")
        || raw.contains(['\\', '\0', '{', '}', '<', '>'])
        || raw.chars().any(char::is_control)
    {
        return None;
    }
    let raw = raw.split(['?', '#']).next()?;
    let mut parts = Vec::new();
    for part in raw.split('/').skip(1) {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    let path = format!("/{}", parts.join("/"));
    Some(if path.is_empty() { "/".into() } else { path })
}

pub(crate) fn join(base: &str, suffix: &str) -> Option<String> {
    if !suffix.starts_with('/')
        || suffix.starts_with("//")
        || base.contains(['?', '#', '\\', '\0'])
        || suffix.contains(['?', '#', '\\', '\0'])
    {
        return None;
    }
    if base.is_empty() || base == "/" {
        return Some(suffix.into());
    }
    if suffix == "/" {
        return Some(base.trim_end_matches('/').into());
    }
    Some(format!(
        "{}/{}",
        base.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    ))
}

pub(crate) fn route_matches(route: &str, path: &str) -> bool {
    let mut expected = route.split('/');
    let mut actual = path.split('/');
    loop {
        match (expected.next(), actual.next()) {
            (Some("*"), _) => return expected.next().is_none(),
            (Some(segment), Some(value)) if segment == value => {}
            (Some(segment), Some(value))
                if segment.starts_with(':')
                    && !segment.contains(['{', '}', '?', '*'])
                    && !value.is_empty() => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_origins_boundaries_and_static_patterns_remain_distinct() {
        assert_eq!(request_path("/a/../b?x=1#part").as_deref(), Some("/b"));
        assert!(request_path("//example.test/b").is_none());
        assert!(request_path("https://example.test/b").is_none());
        assert!(request_path("relative/b").is_none());
        assert_eq!(join("/api", "/").as_deref(), Some("/api"));
        assert!(route_matches("/users/:id", "/users/7"));
        assert!(!route_matches("/users/:id", "/users/7/edit"));
        assert!(!route_matches("/users", "/users/"));
    }
}
