// Purpose: 실제 package·정규 모듈·경로 후보의 모호하지 않은 의존성 key 구성.
// Pattern: Value Encoding.
// Usage: 언어별 resolver와 증분 무효화가 같은 식별 계약을 사용.
// Related: crates/kodstar-core/src/application/analysis/resolve/indexed_paths.rs, crates/kodstar-core/src/infrastructure/index/invalidation.rs.

pub(crate) fn go_package(package: &str, module: &str) -> String {
    format!("{}:{package}{module}", package.len())
}

pub(crate) fn module_provider(language: &str, package: &str, module: &str) -> String {
    format!(
        "{}:{language}{}:{package}{module}",
        language.len(),
        package.len()
    )
}

pub(crate) fn path_provider(group: &str, bytes: &[u8]) -> String {
    format!("{group}:{}", blake3::hash(bytes).to_hex())
}

pub(crate) fn javascript_path(path: &std::path::Path) -> std::path::PathBuf {
    if path.extension().and_then(|v| v.to_str()).is_some_and(|v| {
        matches!(
            v,
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs"
        )
    }) {
        path.with_extension("")
    } else {
        path.to_path_buf()
    }
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
