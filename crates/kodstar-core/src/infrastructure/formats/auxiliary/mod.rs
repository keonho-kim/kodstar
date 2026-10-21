// Purpose: 보조 형식 선택과 파일 root 사실 생성.
// Pattern: Format Adapter.
// Usage: 분석 worker가 보조 입력 추출 시 호출.
// Related: crates/kodstar-core/src/infrastructure/formats/mod.rs, crates/kodstar-core/src/infrastructure/formats/xml.rs.

mod configuration;
mod go;
mod go_tokens;
mod json;
mod jsonc;
mod lines;
mod packages;
mod pnpm;
mod text;
mod toml;
pub(crate) use configuration::{extract as extract_configuration, recognized as is_configuration};

use std::path::Path;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::facts::{Fact, SyntaxUse};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::source::SourceSpan;
use crate::infrastructure::parse::{ParseSession, file_root};

pub(crate) fn language(path: &Path) -> Language {
    if is_configuration(path) {
        return Language::TypeScript;
    }
    match path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
    {
        "package.json" | "pnpm-workspace.yaml" | "pnpm-workspace.yml" => Language::JavaScript,
        "composer.json" => Language::Php,
        "pyproject.toml" => Language::Python,
        "go.mod" | "go.work" => Language::Go,
        "pom.xml" => Language::Java,
        "Directory.Build.props" | "Directory.Build.targets" => Language::CSharp,
        name if name.starts_with("tsconfig") => Language::TypeScript,
        _ => match path.extension().and_then(|extension| extension.to_str()) {
            Some("sln" | "slnx" | "csproj" | "vbproj" | "fsproj" | "xaml") => Language::CSharp,
            Some("dfm" | "lfm" | "lpk") => Language::Pascal,
            _ => Language::Rust,
        },
    }
}

pub(crate) fn extract(
    parser: &mut ParseSession,
    path: &Path,
    source: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
) -> Result<()> {
    if is_configuration(path) {
        return extract_configuration(parser, path, source, cancel, emit);
    }
    let language = language(path);
    let mut emit = |fact| emit(language, fact);
    file_root(path, source, &mut emit)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        let tree = parser.parse_tree(&tree_sitter_json::LANGUAGE.into(), path, source, cancel)?;
        if name == "package.json" {
            packages::extract(tree.root_node(), source, cancel, &mut emit)?;
        }
        json::json(
            tree.root_node(),
            path,
            source,
            &mut Vec::new(),
            &mut emit,
            cancel,
            0,
        )
    } else if path
        .extension()
        .is_some_and(|extension| extension == "toml")
    {
        let tree =
            parser.parse_tree(&tree_sitter_toml_ng::LANGUAGE.into(), path, source, cancel)?;
        toml::toml(tree.root_node(), path, source, &mut emit, cancel)
    } else if matches!(name, "pnpm-workspace.yaml" | "pnpm-workspace.yml") {
        pnpm::extract(source, &mut emit)
    } else if name == "go.mod" || name == "go.work" {
        go::extract(path, source, cancel, &mut emit)
    } else if path.extension().is_some_and(|extension| extension == "sln") {
        text::solution(source, &mut emit)
    } else if path
        .extension()
        .is_some_and(|extension| extension == "dfm" || extension == "lfm")
    {
        text::form(path, source, &mut emit)
    } else {
        super::xml::extract(parser, path, source, cancel, &mut emit)
    }
}

fn usage(
    expression: String,
    relation: RelationKind,
    location: SourceSpan,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    emit(Fact::Use(SyntaxUse {
        source: 0,
        scope: 0,
        expression,
        relation,
        span: location,
        condition: None,
        type_only: false,
    }))
}
