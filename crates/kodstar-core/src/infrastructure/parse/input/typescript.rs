// Purpose: TypeScript 타입 멤버 사이의 실제 개행·구분자 계약 검증.
// Pattern: Lexical Boundary.
// Usage: parser가 개행으로 분리한 generic call signature를 추출하기 전에 호출.
// Related: vendor/tree-sitter-typescript/patch.json, crates/kodstar-core/src/infrastructure/parse/mod.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use std::path::Path;
use tree_sitter::{Node, Tree};

pub(in crate::infrastructure::parse) fn validate(
    tree: &Tree,
    source: &[u8],
    path: &Path,
    cancel: &Cancellation,
) -> Result<()> {
    let mut cursor = tree.root_node().walk();
    loop {
        cancel.check()?;
        let node = cursor.node();
        if matches!(node.kind(), "object_type" | "interface_body") {
            members(node, source, path)?;
        }
        if node.kind() == "call_signature" {
            let mut children = node.walk();
            let mut end = None;
            for child in node.children(&mut children) {
                if child.kind() == "comment" {
                    continue;
                }
                if child.kind() == "call_signature"
                    && let Some(end) = end
                {
                    let gap = &source[end..child.start_byte()];
                    if !gap.iter().any(|b| matches!(b, b'\r' | b'\n'))
                        && memchr::memmem::find(gap, "\u{2028}".as_bytes()).is_none()
                        && memchr::memmem::find(gap, "\u{2029}".as_bytes()).is_none()
                    {
                        return Err(Error::input(
                            "source_syntax_error",
                            "generic call signatures require a line separator",
                            Some(child.start_position().row + 1),
                        ));
                    }
                }
                end = Some(child.end_byte());
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
                return Ok(());
            }
        }
    }
}

fn members(node: Node<'_>, source: &[u8], path: &Path) -> Result<()> {
    let mut cursor = node.walk();
    let mut previous = None::<Node<'_>>;
    let mut separator = false;
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), ";" | ",") {
            separator = true;
        }
        if !child.is_named() || child.kind() == "comment" {
            continue;
        }
        if let Some(previous) = previous {
            let gap = &source[previous.end_byte()..child.start_byte()];
            let terminated = source
                .get(previous.end_byte().saturating_sub(1))
                .is_some_and(|b| matches!(b, b';' | b','));
            if !separator
                && !terminated
                && !gap.iter().any(|b| matches!(b, b'\r' | b'\n'))
                && memchr::memmem::find(gap, "\u{2028}".as_bytes()).is_none()
                && memchr::memmem::find(gap, "\u{2029}".as_bytes()).is_none()
            {
                return Err(Error::input(
                    "source_syntax_error",
                    format!(
                        "missing type member separator in {} at {}:{}",
                        path.display(),
                        child.start_position().row + 1,
                        child.start_position().column + 1
                    ),
                    Some(child.start_position().row + 1),
                ));
            }
        }
        previous = Some(child);
        separator = false;
    }
    Ok(())
}
