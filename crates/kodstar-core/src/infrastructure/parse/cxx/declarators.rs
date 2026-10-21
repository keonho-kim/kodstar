// Purpose: C/C++ declarator and qualified-name AST navigation.
// Pattern: Simple Module.
// Usage: C/C++ extraction and shared type/parameter traversal use these nodes.
// Related: crates/kodstar-core/src/infrastructure/parse/cxx/mod.rs.

use tree_sitter::Node;

pub(in crate::infrastructure::parse) fn declarator(mut node: Node<'_>) -> Option<Node<'_>> {
    for _ in 0..64 {
        if node.kind() == "function_declarator" {
            if node
                .child_by_field_name("declarator")
                .is_some_and(|n| n.kind() == "parenthesized_declarator")
            {
                return None;
            }
            return Some(node);
        }
        node = node.child_by_field_name("declarator")?;
    }
    None
}

pub(in crate::infrastructure::parse) fn qualified_name(node: Node<'_>) -> Option<Node<'_>> {
    declarator(node)?
        .child_by_field_name("declarator")
        .filter(|n| n.kind() == "qualified_identifier")
}

pub(in crate::infrastructure::parse) fn leaf(mut node: Node<'_>) -> Node<'_> {
    for _ in 0..64 {
        if node.kind() != "qualified_identifier" {
            break;
        }
        let Some(name) = node.child_by_field_name("name") else {
            break;
        };
        node = name;
    }
    node
}
