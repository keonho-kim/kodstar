// Purpose: npm workspace·로컬 의존성·조건별 exports/imports 사실 추출.
// Pattern: Format Adapter.
// Usage: JSON manifest adapter가 package.json 트리를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/json.rs, crates/kodstar-core/src/application/analysis/resolve/javascript/packages.rs.

use super::json::{json_string, property};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::{Binding, Fact};
use crate::infrastructure::parse::span;
use tree_sitter::Node;

pub(super) fn extract(
    root: Node<'_>,
    source: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    let Some(root) = root.named_child(0).filter(|n| n.kind() == "object") else {
        return Ok(());
    };
    binding("package_manifest", "npm", None, None, root, emit)?;
    if let Some(workspaces) = property(root, "workspaces", source)? {
        binding("workspace_root", "npm", None, None, workspaces, emit)?;
        let packages = if workspaces.kind() == "object" {
            property(workspaces, "packages", source)?
        } else {
            Some(workspaces)
        };
        if let Some(packages) = packages {
            let mut cursor = packages.walk();
            for pattern in packages
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "string")
            {
                binding(
                    "workspace_glob",
                    &json_string(pattern, source)?,
                    None,
                    None,
                    pattern,
                    emit,
                )?;
            }
        }
    }
    for name in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(dependencies) = property(root, name, source)? {
            let mut pairs = dependencies.walk();
            for pair in dependencies
                .named_children(&mut pairs)
                .filter(|n| n.kind() == "pair")
            {
                let (Some(key), Some(value)) = (
                    pair.child_by_field_name("key"),
                    pair.child_by_field_name("value"),
                ) else {
                    continue;
                };
                if value.kind() != "string" {
                    continue;
                }
                let version = json_string(value, source)?;
                if let Some(path) = version
                    .strip_prefix("file:")
                    .or_else(|| version.strip_prefix("link:"))
                {
                    binding(
                        "package_link",
                        &json_string(key, source)?,
                        Some(path.into()),
                        None,
                        value,
                        emit,
                    )?;
                }
            }
        }
    }
    for name in ["source", "module", "main", "types", "typings"] {
        if let Some(value) = property(root, name, source)?.filter(|n| n.kind() == "string") {
            binding(
                "package_entry",
                name,
                Some(json_string(value, source)?),
                None,
                value,
                emit,
            )?;
        }
    }
    if let Some(exports) = property(root, "exports", source)? {
        binding(
            "package_exports_marker",
            "exports",
            None,
            None,
            exports,
            emit,
        )?;
        if exports.kind() == "object" {
            let mut cursor = exports.walk();
            let mut subpaths = false;
            for pair in exports
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "pair")
            {
                let (Some(key), Some(value)) = (
                    pair.child_by_field_name("key"),
                    pair.child_by_field_name("value"),
                ) else {
                    continue;
                };
                let key = json_string(key, source)?;
                if key.starts_with('.') {
                    subpaths = true;
                    Targets {
                        source,
                        cancel,
                        emit,
                    }
                    .walk("package_export", &key, value, "", 0)?;
                }
            }
            if !subpaths {
                Targets {
                    source,
                    cancel,
                    emit,
                }
                .walk("package_export", ".", exports, "", 0)?;
            }
        } else {
            Targets {
                source,
                cancel,
                emit,
            }
            .walk("package_export", ".", exports, "", 0)?;
        }
    }
    if let Some(imports) = property(root, "imports", source)? {
        let mut cursor = imports.walk();
        for pair in imports
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "pair")
        {
            if let (Some(key), Some(value)) = (
                pair.child_by_field_name("key"),
                pair.child_by_field_name("value"),
            ) {
                Targets {
                    source,
                    cancel,
                    emit,
                }
                .walk(
                    "package_import",
                    &json_string(key, source)?,
                    value,
                    "",
                    0,
                )?;
            }
        }
    }
    Ok(())
}

struct Targets<'a> {
    source: &'a [u8],
    cancel: &'a Cancellation,
    emit: &'a mut dyn FnMut(Fact) -> Result<()>,
}

impl Targets<'_> {
    fn walk(
        &mut self,
        kind: &'static str,
        pattern: &str,
        node: Node<'_>,
        rank: &str,
        depth: usize,
    ) -> Result<()> {
        self.cancel.check()?;
        if depth >= 32 {
            return Err(Error::new(
                ErrorKind::Limit,
                "package condition nesting exceeds 32 levels",
            ));
        }
        match node.kind() {
            "string" => binding(
                kind,
                pattern,
                Some(json_string(node, self.source)?),
                Some(rank.into()),
                node,
                self.emit,
            )?,
            "array" => {
                let mut cursor = node.walk();
                for (index, value) in node
                    .named_children(&mut cursor)
                    .filter(|n| n.kind() != "comment")
                    .enumerate()
                {
                    self.walk(
                        kind,
                        pattern,
                        value,
                        &format!("{rank}a{index:08x}/"),
                        depth + 1,
                    )?;
                }
            }
            "object" => {
                let mut cursor = node.walk();
                for pair in node
                    .named_children(&mut cursor)
                    .filter(|n| n.kind() == "pair")
                {
                    let (Some(key), Some(value)) = (
                        pair.child_by_field_name("key"),
                        pair.child_by_field_name("value"),
                    ) else {
                        continue;
                    };
                    let condition = json_string(key, self.source)?;
                    let priority = match condition.as_str() {
                        "react-native" => "N",
                        "source" => "1",
                        "import" => "2",
                        "module" => "3",
                        "svelte" => "4",
                        "require" => "5",
                        "default" => "6",
                        "types" => "7",
                        _ => continue,
                    };
                    self.walk(
                        kind,
                        pattern,
                        value,
                        &format!("{rank}{priority}/"),
                        depth + 1,
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub(super) fn binding(
    kind: &'static str,
    name: &str,
    module: Option<String>,
    priority: Option<String>,
    node: Node<'_>,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    emit(Fact::Binding(Binding {
        scope: 0,
        name: name.into(),
        target: None,
        module,
        imported_name: priority,
        type_name: None,
        kind,
        span: span(node),
    }))
}
