// Purpose: 완성된 parse tree의 구문 오류·취소 검증.
// Pattern: Parser Validation.
// Usage: ParseSession이 저장·추출 전에 완성된 tree를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/tests/cases/kotlin.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use std::path::Path;
use tree_sitter::Tree;

pub(super) fn validate(tree: &Tree, path: &Path, cancel: &Cancellation) -> Result<()> {
    if !tree.root_node().has_error() {
        return Ok(());
    }
    let mut cursor = tree.root_node().walk();
    let mut steps = 0usize;
    loop {
        if steps.is_multiple_of(128) {
            cancel.check()?;
        }
        steps += 1;
        let node = cursor.node();
        let mut children = node.walk();
        let child_error = node
            .children(&mut children)
            .any(|n| n.has_error() || n.is_error() || n.is_missing());
        if node.is_missing() || node.is_error() && !child_error {
            return Err(Error::input(
                "source_syntax_error",
                format!(
                    "syntax error in {} at {}:{} ({})",
                    path.display(),
                    node.start_position().row + 1,
                    node.start_position().column + 1,
                    if node.is_missing() {
                        node.kind()
                    } else {
                        "ERROR"
                    }
                ),
                Some(node.start_position().row + 1),
            ));
        } else if node.has_error() {
            let mut children = node.walk();
            if !node
                .children(&mut children)
                .any(|n| n.has_error() || n.is_missing() || n.is_error())
            {
                return Err(Error::input(
                    "source_syntax_error",
                    format!("incomplete parse in {}", path.display()),
                    Some(node.start_position().row + 1),
                ));
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Err(Error::input(
                    "source_syntax_error",
                    format!("incomplete parse in {}", path.display()),
                    Some(1),
                ));
            }
        }
    }
}

pub(super) fn contains_rendering(source: &[u8], range: std::ops::Range<usize>) -> bool {
    source
        .get(range.clone())
        .is_some_and(|value| value.windows(2).any(|w| w == b"<%"))
        || range.end > range.start
            && source.get(range.end - 1..range.end.saturating_add(1)) == Some(b"<%")
}

pub(super) fn validate_rendered_names(
    tree: &Tree,
    source: &[u8],
    path: &Path,
    cancel: &Cancellation,
) -> Result<()> {
    let mut cursor = tree.root_node().walk();
    loop {
        cancel.check()?;
        let node = cursor.node();
        if (matches!(
            node.kind(),
            "identifier"
                | "type_identifier"
                | "property_identifier"
                | "private_property_identifier"
                | "shorthand_property_identifier"
                | "shorthand_property_identifier_pattern"
                | "statement_identifier"
                | "computed_property_name"
        ) || node.kind() == "string"
            && node
                .parent()
                .is_some_and(|p| matches!(p.kind(), "import_specifier" | "export_specifier")))
            && contains_rendering(source, node.byte_range())
        {
            return Err(Error::input(
                "unsupported_source_construct",
                format!(
                    "runtime-generated client identifier in {} at {}:{}",
                    path.display(),
                    node.start_position().row + 1,
                    node.start_position().column + 1
                ),
                Some(node.start_position().row + 1),
            ));
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok(());
            }
        }
    }
}
