// Purpose: native 선언·매개변수·매크로 함수의 문맥과 보정 근거를 식별.
// Pattern: Syntax Evidence.
// Usage: native adapter가 원래 파싱 실패의 선언부에만 추가 표식을 선택.
// Related: crates/kodstar-core/src/infrastructure/parse/input/native.rs.

use super::native_tokens::{
    call_end, control_keyword, macro_name, parameter_tail, previous_word, statement_boundary,
};
use tree_sitter::Node;

pub(super) fn misplaced_declaration_macro(node: Node<'_>, source: &[u8]) -> Option<usize> {
    if matches!(node.kind(), "class_specifier" | "struct_specifier")
        && (node.has_error() || node.parent().is_some_and(|parent| parent.has_error()))
        && !node
            .parent()
            .is_some_and(|parent| parent.kind() == "type_definition")
        && let Some(name) = node.child_by_field_name("name")
        && macro_name(&source[name.byte_range()])
        && {
            let rest = source[name.end_byte()..].trim_ascii_start();
            rest.starts_with(b"(")
                || rest
                    .first()
                    .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
                    && !rest.starts_with(b"final")
        }
    {
        return Some(name.start_byte());
    }
    if node.kind() != "function_definition" || !node.has_error() {
        return None;
    }
    let declarator = node.child_by_field_name("declarator")?;
    if declarator.kind() != "identifier" {
        return None;
    }
    let kind = node.child_by_field_name("type")?;
    if macro_name(&source[declarator.byte_range()])
        && source[declarator.end_byte()..]
            .trim_ascii_start()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
    {
        return Some(declarator.start_byte());
    }
    if matches!(
        &source[declarator.byte_range()],
        b"class" | b"struct" | b"namespace"
    ) && macro_name(&source[kind.byte_range()])
    {
        return Some(kind.start_byte());
    }
    if matches!(kind.kind(), "class_specifier" | "struct_specifier") {
        let name = kind.child_by_field_name("name")?;
        if macro_name(&source[name.byte_range()]) {
            return Some(name.start_byte());
        }
    }
    None
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum SyntaxContext {
    Type,
    Statement,
    Declaration,
}

pub(super) fn context(tree: &tree_sitter::Tree, offset: usize) -> SyntaxContext {
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(offset, offset + 1);
    while let Some(current) = node {
        match current.kind() {
            "parameter_declaration"
            | "parameter_list"
            | "type_descriptor"
            | "template_parameter_list" => {
                return SyntaxContext::Type;
            }
            "function_declarator" | "lambda_declarator"
                if current
                    .child_by_field_name("parameters")
                    .is_some_and(|p| offset >= p.end_byte()) =>
            {
                return SyntaxContext::Type;
            }
            "function_definition" | "lambda_expression"
                if current
                    .child_by_field_name("declarator")
                    .is_some_and(|d| offset >= d.end_byte())
                    && current
                        .child_by_field_name("body")
                        .is_some_and(|b| offset < b.start_byte()) =>
            {
                return SyntaxContext::Type;
            }
            "compound_statement" => return SyntaxContext::Statement,
            "field_declaration_list" | "namespace_definition" => return SyntaxContext::Declaration,
            _ => node = current.parent(),
        }
    }
    SyntaxContext::Declaration
}

pub(super) fn macro_function(node: Node<'_>, source: &[u8]) -> Option<usize> {
    if node.kind() != "function_definition" || node.child_by_field_name("type").is_some() {
        return None;
    }
    let name = super::super::syntax::name_node(node)?;
    let declarator = node.child_by_field_name("declarator")?;
    if control_keyword(&source[name.byte_range()])
        || (!macro_name(&source[name.byte_range()])
            && (declarator.kind() != "function_declarator"
                || !statement_boundary(source, name.start_byte())
                || !source[name.end_byte()..]
                    .trim_ascii_start()
                    .starts_with(b"(")))
    {
        return None;
    }
    if declarator
        .child_by_field_name("declarator")
        .is_some_and(|name| name.kind() != "identifier")
    {
        return None;
    }
    if name.kind() != "identifier"
        || previous_word(source, name.start_byte()) == b"if"
        || source[..name.start_byte()]
            .trim_ascii_end()
            .last()
            .is_some_and(|byte| matches!(byte, b')' | b':' | b',' | b'='))
    {
        return None;
    }
    let mut parent = node.parent();
    while let Some(current) = parent {
        match current.kind() {
            "field_declaration_list" | "field_initializer_list" => return None,
            "compound_statement" | "declaration_list" | "translation_unit" => {
                return Some(name.start_byte());
            }
            _ => parent = current.parent(),
        }
    }
    None
}

pub(in crate::infrastructure::parse) fn has_macro_functions(
    tree: &tree_sitter::Tree,
    source: &[u8],
    cancel: &crate::domain::cancellation::Cancellation,
) -> crate::domain::diagnostics::Result<bool> {
    let mut cursor = tree.root_node().walk();
    loop {
        cancel.check()?;
        if macro_function(cursor.node(), source).is_some() {
            return Ok(true);
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok(false);
            }
        }
    }
}

pub(super) fn parameter_suffix(node: Node<'_>, source: &[u8]) -> Option<usize> {
    if node.kind() != "parameter_declaration" || !node.has_error() {
        return None;
    }
    let declarator = node.child_by_field_name("declarator")?;
    let name = declarator.named_descendant_for_byte_range(
        declarator.end_byte().saturating_sub(1),
        declarator.end_byte(),
    )?;
    (name.kind() == "identifier"
        && macro_name(&source[name.byte_range()])
        && parameter_tail(source, name.end_byte()))
    .then_some(name.start_byte())
}

pub(super) fn parameter_prefix(node: Node<'_>, source: &[u8]) -> Option<usize> {
    if node.kind() != "parameter_declaration"
        || !node
            .parent()
            .is_some_and(|p| p.kind() == "parameter_list" && p.has_error())
    {
        return None;
    }
    let kind = node.child_by_field_name("type")?;
    (macro_name(&source[kind.byte_range()])
        && source[kind.end_byte()..]
            .trim_ascii_start()
            .starts_with(b"("))
    .then_some(kind.start_byte())
}

pub(super) fn function_suffix(
    node: Node<'_>,
    source: &[u8],
    cancel: &crate::domain::cancellation::Cancellation,
) -> crate::domain::diagnostics::Result<Option<usize>> {
    let destructor_call = node.kind() == "call_expression"
        && node
            .child_by_field_name("function")
            .is_some_and(|function| {
                function
                    .child_by_field_name("name")
                    .is_some_and(|name| name.kind() == "destructor_name")
            });
    if node.kind() != "function_declarator" && !destructor_call {
        return Ok(None);
    }
    let rest = source[node.end_byte()..].trim_ascii_start();
    let count = rest
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
        .count();
    if !macro_name(&rest[..count]) {
        return Ok(None);
    }
    let start = source.len() - rest.len();
    let Some(end) = call_end(source, start + count, cancel)? else {
        return Ok(None);
    };
    Ok(matches!(
        source[end..].trim_ascii_start().first(),
        Some(b';' | b'{' | b'=')
    )
    .then_some(start))
}

pub(super) fn initializer_modifier(node: Node<'_>, source: &[u8]) -> Option<usize> {
    if node.kind() != "field_initializer"
        && !(node.is_error()
            && node
                .parent()
                .is_some_and(|p| p.kind() == "field_initializer_list"))
    {
        return None;
    }
    let name = node.named_child(0)?;
    if !macro_name(&source[name.byte_range()]) {
        return None;
    }
    let mut cursor = node.walk();
    let args = node
        .named_children(&mut cursor)
        .find(|n| n.kind() == "argument_list")?;
    source[args.end_byte()..]
        .trim_ascii_start()
        .first()
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        .then_some(name.start_byte())
}
