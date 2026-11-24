// Purpose: native 실패 AST에서 오류 범위·정상 선언·구조 매크로의 근거를 수집.
// Pattern: Syntax Evidence.
// Usage: native 입력 adapter가 어휘 표식 선택 전에 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/input/native.rs, crates/kodstar-core/src/infrastructure/parse/input/native_headers.rs.

use super::native_ranges::{coalesce, contains};
use super::native_tokens::{macro_name, tail};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::ops::Range;
use tree_sitter::Tree;

pub(super) struct Evidence {
    pub directives: Vec<Range<usize>>,
    pub errors: Vec<Range<usize>>,
    pub types: Vec<Range<usize>>,
    pub protected: Vec<Range<usize>>,
    pub explicit: Vec<usize>,
    pub statements: Vec<usize>,
    pub structural: Vec<usize>,
    pub functions: Vec<(Range<usize>, Range<usize>)>,
    pub macros: Vec<Range<usize>>,
    pub modifiers: Vec<usize>,
    pub suffixes: Vec<usize>,
    pub qualifiers: Vec<Range<usize>>,
}

pub(super) fn collect(
    tree: &Tree,
    source: &[u8],
    limit: usize,
    cancel: &Cancellation,
) -> Result<Evidence> {
    let mut errors = Vec::<Range<usize>>::new();
    let mut types = super::native_types::lexical(source, limit, cancel)?;
    let mut protected = Vec::<Range<usize>>::new();
    let mut explicit = Vec::<usize>::new();
    let mut statements = Vec::new();
    let mut structural = Vec::new();
    let mut functions = Vec::<(Range<usize>, Range<usize>)>::new();
    let super::native_definitions::Definitions {
        names: mut macros,
        directives,
    } = super::native_definitions::collect(source, limit, cancel)?;
    let mut modifiers = Vec::new();
    let mut suffixes = Vec::new();
    let mut qualifiers = Vec::new();
    let mut cursor = tree.root_node().walk();
    loop {
        cancel.check()?;
        let node = cursor.node();
        if let Some(name) = super::native_types::declared(node, source)
            && !contains(&directives, name.start)
        {
            super::native_types::remember(&mut types, name, source);
        }
        let wrapper = super::native_headers::macro_function(node, source);
        if let Some(offset) = wrapper {
            structural.push(offset);
            remember(&mut errors, node.byte_range());
        }
        if node.kind() == "type_qualifier"
            && source.get(node.start_byte()) == Some(&super::native::MODIFIER)
        {
            qualifiers.push(node.byte_range());
        }
        if let Some(offset) = super::native_headers::function_suffix(node, source, cancel)? {
            suffixes.push(offset);
        }
        if let Some(offset) = super::native_headers::initializer_modifier(node, source) {
            modifiers.push(offset);
        }
        if let Some(offset) = super::native_headers::misplaced_declaration_macro(node, source) {
            modifiers.push(offset);
        }
        if matches!(
            node.kind(),
            "macro_block" | "macro_statement" | "macro_declaration"
        ) && let Some(name) = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("function"))
            && source.get(name.start_byte()) == Some(&b'@')
        {
            macros.push(name.byte_range());
        }
        if node.kind() == "call_expression"
            && !node.has_error()
            && node.parent().is_some_and(|parent| !parent.has_error())
            && !std::iter::successors(node.parent(), |parent| parent.parent())
                .take_while(|parent| {
                    !matches!(
                        parent.kind(),
                        "declaration" | "field_declaration" | "expression_statement"
                    )
                })
                .any(|parent| {
                    matches!(
                        parent.kind(),
                        "type_descriptor" | "type_definition" | "alias_declaration"
                    )
                })
        {
            remember(&mut protected, node.byte_range());
        }
        if matches!(node.kind(), "preproc_def" | "preproc_function_def") {
            remember(&mut protected, node.byte_range());
        }
        if node.kind() == "call_expression"
            && !node.has_error()
            && node.parent().is_some_and(|parent| parent.is_error())
            && let Some(name) = node
                .child_by_field_name("function")
                .filter(|name| name.kind() == "identifier")
            && tail(source, node.end_byte(), false, cancel)?
        {
            statements.push(name.start_byte());
        }
        if node.is_error() || node.is_missing() {
            let range = node.byte_range();
            let mut affected = if range.is_empty() {
                node.parent().map_or(range.clone(), |p| p.byte_range())
            } else {
                range.clone()
            };
            let line = memchr::memrchr(b'\n', &source[..affected.start]).unwrap_or(0);
            affected.start = memchr::memrchr(b'\n', &source[..line]).map_or(0, |n| n + 1);
            remember(&mut errors, affected);
            if node.is_missing()
                && node.kind() == "::"
                && let Some(parent) = node.parent().filter(|p| p.kind() == "qualified_identifier")
                && let Some(scope) = parent.child_by_field_name("scope")
                && macro_name(&source[scope.byte_range()])
            {
                explicit.push(scope.start_byte());
            }
            if node.is_missing()
                && node.kind() == ";"
                && let Some(parent) = node.parent().filter(|p| p.kind() == "declaration")
                && let Some(name) = parent.child_by_field_name("declarator")
                && name.kind() == "identifier"
                && macro_name(&source[name.byte_range()])
                && parent
                    .child_by_field_name("type")
                    .is_none_or(|kind| !macro_name(&source[kind.byte_range()]))
            {
                explicit.push(name.start_byte());
            }
            if node.is_missing()
                && node.kind() == ";"
                && let Some(parent) = node.parent().filter(|p| p.kind() == "expression_statement")
                && let Some(call) = parent
                    .named_child(0)
                    .filter(|c| c.kind() == "call_expression")
                && let Some(name) = call
                    .child_by_field_name("function")
                    .filter(|n| n.kind() == "identifier")
                && source[name.start_byte()].is_ascii()
                && tail(source, call.end_byte(), false, cancel)?
            {
                statements.push(name.start_byte());
            }
            if macro_name(&source[range.clone()])
                && node.parent().is_some_and(|p| {
                    matches!(
                        p.kind(),
                        "declaration"
                            | "field_declaration"
                            | "function_definition"
                            | "class_specifier"
                            | "struct_specifier"
                            | "pointer_declarator"
                    )
                })
            {
                explicit.push(range.start);
            }
        }
        if matches!(
            node.kind(),
            "function_definition"
                | "declaration"
                | "field_declaration"
                | "type_definition"
                | "type_descriptor"
                | "parameter_declaration"
                | "class_specifier"
                | "struct_specifier"
                | "namespace_definition"
                | "enum_specifier"
                | "template_instantiation"
                | "type_parameter_declaration"
                | "optional_type_parameter_declaration"
                | "variadic_type_parameter_declaration"
                | "enumerator"
        ) {
            let valid_header = node.child_by_field_name("body").is_none_or(|body| {
                let mut children = node.walk();
                node.named_children(&mut children)
                    .filter(|n| n.end_byte() <= body.start_byte())
                    .all(|n| !n.has_error())
            });
            modifiers.extend(
                [
                    super::native_headers::parameter_prefix(node, source),
                    super::native_headers::parameter_suffix(node, source),
                ]
                .into_iter()
                .flatten(),
            );
            if node.has_error()
                && let Some(kind) = node.child_by_field_name("type")
                && macro_name(&source[kind.byte_range()])
                && node.child_by_field_name("body").is_none_or(|body| {
                    let mut children = node.walk();
                    node.named_children(&mut children)
                        .any(|n| n.end_byte() <= body.start_byte() && n.has_error())
                })
            {
                explicit.push(kind.start_byte());
                remember(&mut errors, kind.byte_range());
            }
            if (node.kind() == "function_definition" && valid_header
                || matches!(node.kind(), "declaration" | "field_declaration") && !node.has_error())
                && let Some(kind) = node.child_by_field_name("type")
                && node
                    .child_by_field_name("declarator")
                    .is_some_and(|n| n.kind() == "function_declarator")
                && let Some(name) = super::super::syntax::name_node(node)
                && macro_name(&source[name.byte_range()])
                && !contains(&directives, name.start_byte())
            {
                functions.push((name.byte_range(), kind.byte_range()));
            }
            if wrapper.is_none() {
                let incomplete_alias = node.kind() == "type_descriptor"
                    && node.parent().is_some_and(|parent| parent.has_error());
                if !node.has_error() && !incomplete_alias {
                    remember(&mut protected, node.byte_range());
                } else if let Some(body) = node.child_by_field_name("body")
                    && (node.kind() != "function_definition"
                        || super::super::cxx::declarators::declarator(node).is_some())
                {
                    let mut children = node.walk();
                    if node
                        .named_children(&mut children)
                        .filter(|n| n.end_byte() <= body.start_byte())
                        .all(|n| !n.has_error())
                    {
                        remember(&mut protected, node.start_byte()..body.start_byte());
                    }
                }
            }
        }
        if (directives.len()
            + errors.len()
            + types.len()
            + protected.len()
            + explicit.len()
            + statements.len()
            + structural.len()
            + functions.len()
            + macros.len()
            + modifiers.len()
            + suffixes.len()
            + qualifiers.len())
        .saturating_mul(64)
            > limit
        {
            return Err(Error::new(
                ErrorKind::Limit,
                "native syntax repair workspace exceeded",
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
                break;
            }
        }
        if cursor.node().id() == tree.root_node().id() {
            break;
        }
    }
    protected.extend(directives.iter().cloned());
    Ok(Evidence {
        directives,
        errors: coalesce(errors),
        types,
        protected: coalesce(protected),
        explicit,
        statements,
        structural,
        functions,
        macros,
        modifiers,
        suffixes,
        qualifiers,
    })
}

fn remember(ranges: &mut Vec<Range<usize>>, mut range: Range<usize>) {
    while ranges
        .last()
        .is_some_and(|previous| previous.end >= range.start)
    {
        let previous = ranges.pop().expect("a range was present");
        range.start = range.start.min(previous.start);
        range.end = range.end.max(previous.end);
    }
    ranges.push(range);
}
