// Purpose: 보조 입력 판별과 format adapter 진입점.
// Pattern: Simple Module.
// Usage: scan·분석 worker가 실제 입력별 adapter 선택.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs, crates/kodstar-core/src/infrastructure/formats/components.rs.

//! Source-related auxiliary formats and original-position embedded code.

mod auxiliary;
mod components;
mod dotnet;
mod xml;

pub(crate) use auxiliary::{extract as extract_auxiliary, language as auxiliary_language};
pub(crate) use auxiliary::{extract_configuration, is_configuration};
pub(crate) use components::extract as extract_component;

use std::path::Path;

pub(crate) fn is_auxiliary(path: &Path) -> bool {
    if is_configuration(path) {
        return true;
    }
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            "Cargo.toml"
                | "pnpm-workspace.yaml"
                | "pnpm-workspace.yml"
                | "package.json"
                | "go.mod"
                | "go.work"
                | "pyproject.toml"
                | "pom.xml"
                | "composer.json"
                | "Directory.Build.props"
                | "Directory.Build.targets"
        )
    ) || path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension,
                "sln" | "slnx" | "csproj" | "fsproj" | "vbproj" | "xaml" | "dfm" | "lfm" | "lpk"
            )
        })
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("tsconfig") && name.ends_with(".json"))
}
