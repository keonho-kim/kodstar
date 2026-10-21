// Purpose: 경로 설정·상속·project reference를 순차 구문 사실로 추출.
// Pattern: Format Adapter.
// Usage: 공식 JSON grammar로 파싱한 설정 트리를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs, crates/kodstar-core/src/application/analysis/resolve/javascript/mod.rs.

use super::json::{json_string as string, property};
use super::{jsonc, usage};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{ParseSession, file_root, span};
use std::path::Path;
use tree_sitter::Node;

pub(crate) fn recognized(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            (name.starts_with("tsconfig") || name.starts_with("jsconfig"))
                && (name.ends_with(".json") || name.ends_with(".jsonc"))
        })
}

pub(crate) fn extract(
    parser: &mut ParseSession,
    path: &Path,
    source: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
) -> Result<()> {
    let masked = jsonc::input(source)?;
    let tree = parser.parse_tree(&tree_sitter_json::LANGUAGE.into(), path, &masked, cancel)?;
    let mut cursor = tree.root_node().walk();
    let mut values = tree
        .root_node()
        .named_children(&mut cursor)
        .filter(|n| n.kind() != "comment");
    let root = values
        .next()
        .filter(|n| n.kind() == "object")
        .ok_or_else(|| Error::operation("configuration must be a JSON object"))?;
    if values.next().is_some() {
        return Err(Error::operation(
            "configuration must contain one JSON object",
        ));
    }
    let emit = &mut |fact| emit(Language::TypeScript, fact);
    file_root(path, source, emit)?;
    binding(
        "configuration",
        if recognized(path) {
            "root"
        } else {
            "referenced"
        },
        None,
        None,
        root,
        emit,
    )?;
    if let Some(options) = property(root, "compilerOptions", source)? {
        let base = property(options, "baseUrl", source)?
            .filter(|n| n.kind() == "string")
            .map(|n| string(n, source))
            .transpose()?;
        if let Some(base) = &base {
            binding(
                "base_url",
                "@baseUrl",
                Some(base.clone()),
                None,
                options,
                emit,
            )?;
        }
        if let Some(paths) = property(options, "paths", source)? {
            let mut pairs = paths.walk();
            for pair in paths
                .named_children(&mut pairs)
                .filter(|n| n.kind() == "pair")
            {
                cancel.check()?;
                let (Some(key), Some(value)) = (
                    pair.child_by_field_name("key"),
                    pair.child_by_field_name("value"),
                ) else {
                    continue;
                };
                let pattern = string(key, source)?;
                binding("path_alias_start", &pattern, None, None, pair, emit)?;
                if value.kind() != "array" {
                    return Err(Error::operation(
                        "configuration paths must contain arrays of strings",
                    ));
                }
                let mut targets = value.walk();
                for target in value
                    .named_children(&mut targets)
                    .filter(|n| n.kind() == "string")
                {
                    binding(
                        "path_alias",
                        &pattern,
                        Some(string(target, source)?),
                        base.clone(),
                        target,
                        emit,
                    )?;
                }
            }
        }
    }
    if let Some(extends) = property(root, "extends", source)? {
        if extends.kind() == "string" {
            link("extends", extends, source, emit)?;
        } else if extends.kind() == "array" {
            let mut items = extends.walk();
            for target in extends
                .named_children(&mut items)
                .filter(|n| n.kind() == "string")
            {
                link("extends", target, source, emit)?;
            }
        } else {
            return Err(Error::operation(
                "configuration extends must be a string or array",
            ));
        }
    }
    if let Some(references) = property(root, "references", source)? {
        let mut items = references.walk();
        for reference in references.named_children(&mut items) {
            if let Some(path) =
                property(reference, "path", source)?.filter(|n| n.kind() == "string")
            {
                link("reference", path, source, emit)?;
            }
        }
    }
    Ok(())
}

fn link(
    kind: &'static str,
    node: Node<'_>,
    source: &[u8],
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    let target = string(node, source)?;
    binding(
        "configuration_link",
        kind,
        Some(target.clone()),
        None,
        node,
        emit,
    )?;
    usage(
        format!("@configuration:{kind}:{target}"),
        RelationKind::Includes,
        span(node),
        emit,
    )
}

fn binding(
    kind: &'static str,
    name: &str,
    module: Option<String>,
    base: Option<String>,
    node: Node<'_>,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    emit(Fact::Binding(Binding {
        scope: 0,
        name: name.into(),
        target: None,
        module,
        imported_name: base,
        type_name: None,
        kind,
        span: span(node),
    }))
}
