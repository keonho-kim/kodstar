// Purpose: AST 이름·위치·선언 종류 접근.
// Pattern: Simple Module.
// Usage: parser 추출기와 format adapter가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs.

use crate::domain::source::SourceSpan;
use crate::domain::symbols::SymbolKind;
use tree_sitter::Node;

pub(crate) fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.byte_range()]).unwrap_or("")
}

pub(crate) fn span(node: Node<'_>) -> SourceSpan {
    let start = node.start_position();
    let end = node.end_position();
    SourceSpan {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        line: start.row + 1,
        column: start.column + 1,
        end_line: end.row + 1,
        end_column: end.column + 1,
    }
}

pub(crate) fn child_kind<'tree>(node: Node<'tree>, kinds: &[&str]) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| kinds.contains(&child.kind()))
}

pub(super) fn name_node<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    if let Some(name) = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("function_name"))
    {
        return Some(name);
    }
    if let Some(declarator) = node.child_by_field_name("declarator") {
        let mut current = declarator;
        for _ in 0..64 {
            if matches!(
                current.kind(),
                "identifier"
                    | "field_identifier"
                    | "qualified_identifier"
                    | "operator_name"
                    | "destructor_name"
            ) {
                return Some(current);
            }
            match current
                .child_by_field_name("declarator")
                .or_else(|| current.child_by_field_name("name"))
            {
                Some(next) => current = next,
                None => break,
            }
        }
    }
    child_kind(
        node,
        &[
            "identifier",
            "type_identifier",
            "constant",
            "simple_identifier",
            "name",
            "atom",
        ],
    )
}

pub(super) fn kind_for(node: Node<'_>, function: bool, member: bool) -> SymbolKind {
    let kind = node.kind();
    if kind.contains("constructor") || kind == "init_declaration" {
        SymbolKind::Constructor
    } else if kind.contains("macro") {
        SymbolKind::Macro
    } else if function {
        if member || kind.contains("method") {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        }
    } else if kind.contains("interface") || kind.contains("protocol") {
        SymbolKind::Interface
    } else if kind.contains("trait") {
        SymbolKind::Trait
    } else if kind.contains("enum") {
        SymbolKind::Enum
    } else if kind.contains("struct") {
        SymbolKind::Struct
    } else if kind.contains("alias") || kind == "type_item" {
        SymbolKind::Type
    } else if kind.contains("module") || kind == "mod_item" || kind.contains("namespace") {
        SymbolKind::Module
    } else {
        SymbolKind::Class
    }
}
