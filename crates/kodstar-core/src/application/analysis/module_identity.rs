// Purpose: 보존 manifest와 파일 경로의 package·module 식별.
// Pattern: Simple Module.
// Usage: discovery가 파일 상태 구성 시 호출.
// Related: crates/kodstar-core/src/application/analysis/discovery.rs.

use crate::domain::languages::Language;
use crate::infrastructure::cache::paths::{cache_key, path_bytes};
use std::path::{Path, PathBuf};

pub(super) fn package_path(
    entry: &crate::infrastructure::history::inputs::Entry,
    language: Option<Language>,
) -> crate::domain::diagnostics::Result<PathBuf> {
    let relative = &entry.relative;
    let marker = match language {
        Some(Language::Rust) => "Cargo.toml",
        Some(Language::Go) => "go.mod",
        Some(Language::Python) => "pyproject.toml",
        Some(Language::Php) => "composer.json",
        Some(
            Language::JavaScript
            | Language::TypeScript
            | Language::Vue
            | Language::Svelte
            | Language::Astro,
        ) => "package.json",
        Some(Language::Java | Language::Kotlin | Language::Scala) => "pom.xml",
        Some(Language::Swift) => "Package.swift",
        Some(Language::Elixir) => "mix.exs",
        _ => "",
    };
    let mut package_path = PathBuf::new();
    if !marker.is_empty() {
        for directory in relative
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .ancestors()
        {
            if entry.has_file(&directory.join(marker))? {
                package_path = directory.to_path_buf();
                break;
            }
        }
    }
    Ok(package_path)
}

pub(super) fn module_identity(
    root: &Path,
    relative: &Path,
    language: Option<Language>,
    package_path: &Path,
) -> (String, String) {
    let local = relative.strip_prefix(package_path).unwrap_or(relative);
    let package = cache_key(&root.join(package_path));
    if local.to_str().is_none() {
        return (
            package,
            format!("@path:{}", blake3::hash(&path_bytes(local)).to_hex()),
        );
    }
    let mut module_path = local.with_extension("");
    if matches!(
        language,
        Some(
            Language::Go
                | Language::Java
                | Language::Kotlin
                | Language::Scala
                | Language::CSharp
                | Language::Swift
        )
    ) {
        module_path = local
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
    }
    if language == Some(Language::Python)
        && local.file_stem().is_some_and(|name| name == "__init__")
    {
        module_path.pop();
    }
    if language == Some(Language::Rust) {
        if module_path.starts_with("src") {
            module_path = module_path
                .strip_prefix("src")
                .unwrap_or(&module_path)
                .to_path_buf();
        }
        if matches!(
            module_path.file_name().and_then(|name| name.to_str()),
            Some("lib" | "main" | "mod")
        ) {
            module_path.pop();
        }
        let tail = module_path
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("::");
        return (
            package,
            if tail.is_empty() {
                "crate".into()
            } else {
                format!("crate::{tail}")
            },
        );
    }
    let separator = if matches!(
        language,
        Some(
            Language::Python
                | Language::Java
                | Language::Kotlin
                | Language::Scala
                | Language::CSharp
        )
    ) {
        "."
    } else {
        "/"
    };
    (
        package,
        module_path
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join(separator),
    )
}
