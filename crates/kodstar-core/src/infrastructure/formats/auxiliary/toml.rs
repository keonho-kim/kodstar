// Purpose: TOML manifest의 package·의존·경로 관계 추출.
// Pattern: Format Adapter.
// Usage: auxiliary가 공식 grammar의 TOML 트리를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs.

use std::path::Path;

use tree_sitter::Node;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Binding, Fact};
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{span, text};

use super::usage;

fn toml_string(node: Node<'_>, source: &[u8]) -> Result<String> {
    let fragment = format!("value = {}", text(node, source));
    let values: std::collections::BTreeMap<String, String> = config::Config::builder()
        .add_source(config::File::from_str(&fragment, config::FileFormat::Toml))
        .build()
        .map_err(|error| {
            Error::private_input(
                crate::domain::diagnostics::ErrorKind::Operation,
                "invalid TOML string (input excerpt omitted)",
                error,
            )
        })?
        .try_deserialize()
        .map_err(|error| {
            Error::private_input(
                crate::domain::diagnostics::ErrorKind::Operation,
                "invalid TOML string value (input excerpt omitted)",
                error,
            )
        })?;
    values
        .get("value")
        .cloned()
        .ok_or_else(|| Error::operation("TOML value has no decoded string"))
}

fn toml_keys(node: Node<'_>, source: &[u8], cancel: &Cancellation) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut cursor = node.walk();
    let mut steps = 0usize;
    loop {
        if steps.is_multiple_of(128) {
            cancel.check()?;
        }
        steps += 1;
        let current = cursor.node();
        let leaf = match current.kind() {
            "quoted_key" => {
                result.push(toml_string(current, source)?);
                true
            }
            "bare_key" => {
                result.push(text(current, source).into());
                true
            }
            _ => false,
        };
        if !leaf && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok(result);
            }
        }
    }
}

fn pair_value<'a>(node: Node<'a>, name: &str, source: &[u8]) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|node| node.kind() == "pair")
        .find(|pair| {
            pair.named_child(0)
                .is_some_and(|key| text(key, source).trim_matches(['\'', '"']) == name)
        })
        .and_then(|pair| pair.named_child(1))
}

pub(super) fn toml(
    root: Node<'_>,
    path: &Path,
    source: &[u8],
    emit: &mut dyn FnMut(Fact) -> Result<()>,
    cancel: &Cancellation,
) -> Result<()> {
    let cargo = path.file_name().is_some_and(|name| name == "Cargo.toml");
    let mut cursor = root.walk();
    for table in root.named_children(&mut cursor) {
        cancel.check()?;
        if !matches!(table.kind(), "table" | "table_array_element") {
            continue;
        }
        let Some(header) = table.named_child(0) else {
            continue;
        };
        let keys = toml_keys(header, source, cancel)?;
        if (keys == ["package"] || keys == ["project"])
            && let Some(name) = pair_value(table, "name", source)
        {
            emit(Fact::Module {
                name: toml_string(name, source)?,
                declaration_only: true,
            })?;
        }
        let dependency_index = keys.iter().position(|key| {
            matches!(
                key.as_str(),
                "dependencies" | "dev-dependencies" | "build-dependencies"
            )
        });
        if cargo {
            if let Some(index) = dependency_index {
                if let Some(name) = keys.get(index + 1) {
                    cargo_dependency(name, table, header, source, emit)?;
                } else {
                    let mut cursor = table.walk();
                    for pair in table
                        .named_children(&mut cursor)
                        .filter(|node| node.kind() == "pair")
                    {
                        if let (Some(key), Some(value)) = (pair.named_child(0), pair.named_child(1))
                        {
                            let names = toml_keys(key, source, cancel)?;
                            if let Some(name) = names.first() {
                                cargo_dependency(name, value, key, source, emit)?;
                            }
                        }
                    }
                }
            }
        } else if keys == ["project"]
            && let Some(dependencies) = pair_value(table, "dependencies", source)
        {
            let mut cursor = dependencies.walk();
            for value in dependencies
                .named_children(&mut cursor)
                .filter(|node| node.kind() == "string")
            {
                let value_text = toml_string(value, source)?;
                let name = value_text
                    .split(|character: char| {
                        !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_' | '.')
                    })
                    .next()
                    .unwrap_or("");
                if !name.is_empty() {
                    usage(
                        format!("@package:pypi:{name}"),
                        RelationKind::DependsOn,
                        span(value),
                        emit,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn cargo_dependency(
    name: &str,
    value: Node<'_>,
    location: Node<'_>,
    source: &[u8],
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    let actual = pair_value(value, "package", source)
        .map(|node| toml_string(node, source))
        .transpose()?
        .unwrap_or_else(|| name.into());
    usage(
        format!("@package:cargo:{actual}"),
        RelationKind::DependsOn,
        span(location),
        emit,
    )?;
    emit(Fact::Binding(Binding {
        scope: 0,
        name: name.replace('-', "_"),
        target: None,
        module: Some(actual),
        imported_name: None,
        type_name: None,
        kind: "dependency_alias",
        span: span(location),
    }))?;
    if let Some(path) = pair_value(value, "path", source) {
        usage(
            format!("@file:{}/Cargo.toml", toml_string(path, source)?),
            RelationKind::DependsOn,
            span(path),
            emit,
        )?;
    }
    Ok(())
}
