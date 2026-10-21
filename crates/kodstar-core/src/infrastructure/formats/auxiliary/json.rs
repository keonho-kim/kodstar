// Purpose: JSON manifest의 의존·export·경로 관계 추출.
// Pattern: Format Adapter.
// Usage: auxiliary가 공식 grammar의 JSON 트리를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs.

use std::path::Path;

use tree_sitter::Node;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::Fact;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::span;

use super::usage;

pub(super) fn json_string(node: Node<'_>, source: &[u8]) -> Result<String> {
    serde_json::from_slice(&source[node.byte_range()])
        .map_err(|error| Error::caused_by("decode manifest string", error))
}

pub(super) fn json(
    node: Node<'_>,
    path: &Path,
    source: &[u8],
    keys: &mut Vec<String>,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
    cancel: &Cancellation,
    depth: usize,
) -> Result<()> {
    if depth > 512 {
        return Err(Error::operation("manifest nesting exceeds 512 levels"));
    }
    cancel.check()?;
    let pair = node.kind() == "pair";
    if pair {
        let key = node
            .child_by_field_name("key")
            .ok_or_else(|| Error::operation("JSON pair has no key"))?;
        keys.push(json_string(key, source)?);
        let value = node
            .child_by_field_name("value")
            .ok_or_else(|| Error::operation("JSON pair has no value"))?;
        let ecosystem = if path.file_name().is_some_and(|name| name == "composer.json") {
            "composer"
        } else {
            "npm"
        };
        if keys.len() == 1 && keys[0] == "name" && value.kind() == "string" {
            emit(Fact::Module {
                name: json_string(value, source)?,
                declaration_only: true,
            })?;
        }
        if keys.len() == 2
            && matches!(
                keys[0].as_str(),
                "dependencies"
                    | "devDependencies"
                    | "peerDependencies"
                    | "optionalDependencies"
                    | "require"
                    | "require-dev"
            )
        {
            usage(
                format!("@package:{ecosystem}:{}", keys[1]),
                RelationKind::DependsOn,
                span(key),
                emit,
            )?;
            if value.kind() == "string" {
                let version = json_string(value, source)?;
                if let Some(local) = version
                    .strip_prefix("file:")
                    .or_else(|| version.strip_prefix("link:"))
                {
                    usage(
                        format!("@file:{local}/package.json"),
                        RelationKind::DependsOn,
                        span(value),
                        emit,
                    )?;
                }
            }
        }
        if value.kind() == "string"
            && keys.first().is_some_and(|key| {
                matches!(
                    key.as_str(),
                    "main" | "module" | "types" | "exports" | "imports" | "extends"
                )
            })
        {
            let target = json_string(value, source)?;
            if target.starts_with('.') {
                usage(
                    format!(
                        "{}:{target}",
                        if matches!(keys[0].as_str(), "exports" | "imports") {
                            "@packagefile"
                        } else {
                            "@file"
                        }
                    ),
                    RelationKind::Includes,
                    span(value),
                    emit,
                )?;
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        json(child, path, source, keys, emit, cancel, depth + 1)?;
    }
    if pair {
        keys.pop();
    }
    Ok(())
}

pub(super) fn property<'tree>(
    node: Node<'tree>,
    name: &str,
    source: &[u8],
) -> Result<Option<Node<'tree>>> {
    let mut result = None;
    let mut cursor = node.walk();
    for pair in node
        .named_children(&mut cursor)
        .filter(|n| n.kind() == "pair")
    {
        if let Some(key) = pair.child_by_field_name("key")
            && json_string(key, source)? == name
        {
            result = pair.child_by_field_name("value");
        }
    }
    Ok(result)
}
