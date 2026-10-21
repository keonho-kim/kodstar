// Purpose: PHP grouped use와 class/function/const import 별칭 추출.
// Pattern: Simple Module.
// Usage: PHP namespace_use_clause를 import dispatcher가 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs, crates/kodstar-core/src/application/analysis/resolve/php/mod.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{child_kind, span, text};
use tree_sitter::Node;

fn category(node: Node<'_>) -> Option<&'static str> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find_map(|n| match n.kind() {
        "function" => Some("php_import_function"),
        "const" => Some("php_import_value"),
        _ => None,
    })
}

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn php(&mut self, node: Node<'_>) -> Result<()> {
        let Some(name) = child_kind(node, &["qualified_name", "name", "relative_name"]) else {
            return Ok(());
        };
        let mut path = String::new();
        let parent = node.parent();
        let declaration = parent.and_then(|p| {
            if p.kind() == "namespace_use_group" {
                p.parent()
            } else {
                Some(p)
            }
        });
        if parent.is_some_and(|p| p.kind() == "namespace_use_group")
            && let Some(prefix) = declaration.and_then(|p| child_kind(p, &["namespace_name"]))
        {
            path.push_str(text(prefix, self.source).trim_matches('\\'));
            path.push('\\');
        }
        let raw = text(name, self.source).trim_start_matches('\\');
        if path.len().saturating_add(raw.len()) > self.limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "PHP import path exceeds fact_batch_bytes",
            ));
        }
        path.push_str(raw);
        let alias = node
            .child_by_field_name("alias")
            .map(|n| text(n, self.source))
            .unwrap_or_else(|| raw.rsplit('\\').next().unwrap_or(raw));
        self.binding_kind = Some(
            category(node)
                .or_else(|| declaration.and_then(category))
                .unwrap_or("php_import_type"),
        );
        self.bind(alias, &path, None, span(node))
    }
}
