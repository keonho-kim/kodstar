// Purpose: 실제 Go package의 모호하지 않은 의존성 key 구성.
// Pattern: Value Encoding.
// Usage: Go resolver와 증분 무효화가 같은 식별 계약을 사용.
// Related: crates/kodstar-core/src/application/analysis/resolve/go/mod.rs, crates/kodstar-core/src/infrastructure/index/invalidation.rs.

pub(crate) fn go_package(package: &str, module: &str) -> String {
    format!("{}:{package}{module}", package.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separators_and_unicode_do_not_merge_distinct_packages() {
        assert_ne!(go_package("a:b", "c"), go_package("a", "b:c"));
        assert_ne!(go_package("별", "api"), go_package("별a", "pi"));
    }
}
