// Purpose: 원본 C/C++ 입력이 내부 매크로 표식을 구문으로 주입하지 못하게 검사.
// Pattern: Input Boundary.
// Usage: native parser가 보정 후 원래 바이트와 AST의 표식 위치를 대조.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/infrastructure/parse/input/native.rs.

use super::native::MODIFIER;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use std::path::Path;
use tree_sitter::Tree;

pub(in crate::infrastructure::parse) fn validate_original(
    tree: &Tree,
    path: &Path,
    source: &[u8],
    cancel: &Cancellation,
) -> Result<()> {
    if !source.contains(&b'@') && !source.contains(&MODIFIER) {
        return Ok(());
    }
    let mut cursor = tree.root_node().walk();
    loop {
        cancel.check()?;
        let node = cursor.node();
        if matches!(node.kind(), "identifier" | "type_qualifier")
            && source
                .get(node.start_byte())
                .is_some_and(|b| matches!(*b, b'@' | MODIFIER))
        {
            return Err(Error::input(
                "source_syntax_error",
                format!(
                    "invalid native syntax marker in {} at {}:{}",
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
