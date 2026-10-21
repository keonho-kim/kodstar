// Purpose: 매개변수·변수·alias의 lexical binding 추출.
// Pattern: Simple Module.
// Usage: 선언과 참조 추출이 바인딩 정보를 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/infrastructure/parse/references.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn variable(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        if self.python_binding(node, context)? {
            return Ok(());
        }
        if self.php_binding(node, context)? {
            return Ok(());
        }
        if self.native_result_binding(node, context)? {
            return Ok(());
        }
        if self.ruby_binding(node, context)? {
            return Ok(());
        }
        if self.pascal_binding(node, context)? {
            return Ok(());
        }
        if self.kotlin_binding(node, context)? {
            return Ok(());
        }
        if self.swift_binding(node, context)? {
            return Ok(());
        }
        if self.scala_binding(node, context)? {
            return Ok(());
        }
        if crate::infrastructure::parse::fields::field_variable(node) {
            return Ok(());
        }
        if self.language == Language::Go && node.kind() == "short_var_declaration" {
            return self.go_short_binding(node, context);
        }
        if !matches!(
            node.kind(),
            "let_declaration"
                | "variable_declarator"
                | "assignment"
                | "var_spec"
                | "short_var_declaration"
                | "property_declaration"
        ) {
            return Ok(());
        }
        let name = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("pattern"))
            .or_else(|| node.child_by_field_name("left"));
        let Some(name) = name else { return Ok(()) };
        if !matches!(
            name.kind(),
            "identifier" | "variable_name" | "simple_identifier"
        ) {
            return Ok(());
        }
        let value = node
            .child_by_field_name("value")
            .or_else(|| node.child_by_field_name("right"));
        if matches!(self.language, Language::JavaScript | Language::TypeScript)
            && value.is_some_and(|value| {
                crate::infrastructure::parse::javascript::commonjs::require_module(
                    value,
                    self.source,
                )
                .is_some()
            })
        {
            return Ok(());
        }
        if value.is_some_and(|value| {
            matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "anonymous_function"
            )
        }) {
            return Ok(());
        }
        let annotation = node
            .child_by_field_name("type")
            .map(|node| self.type_text(node))
            .transpose()?;
        let call_result = if self.language == Language::Python {
            value
                .filter(|n| n.kind() == "call")
                .and_then(|n| n.child_by_field_name("function"))
                .map(|n| self.reference_text(n))
                .transpose()?
        } else {
            None
        };
        let alias = value
            .filter(|value| {
                matches!(
                    value.kind(),
                    "identifier" | "scoped_identifier" | "attribute" | "member_expression"
                )
            })
            .map(|value| self.reference_text(value))
            .transpose()?;
        let scope = if matches!(self.language, Language::JavaScript | Language::TypeScript)
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "variable_declaration")
        {
            context.function_scope
        } else {
            context.scope
        };
        (self.emit)(Fact::Binding(Binding {
            scope,
            name: text(name, self.source).trim_start_matches('$').into(),
            target: None,
            module: None,
            imported_name: alias.clone().or(call_result.clone()),
            type_name: annotation,
            kind: if call_result.is_some() {
                "call_result"
            } else if alias.is_some() {
                "alias"
            } else {
                "local"
            },
            span: span(name),
        }))
    }

    fn go_short_binding(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        let (Some(names), Some(values)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return Ok(());
        };
        let mut name_cursor = names.walk();
        let mut value_cursor = values.walk();
        for (name, value) in names
            .named_children(&mut name_cursor)
            .zip(values.named_children(&mut value_cursor))
        {
            if name.kind() != "identifier" {
                continue;
            }
            let value = if value.kind() == "unary_expression" {
                value.child_by_field_name("operand").unwrap_or(value)
            } else {
                value
            };
            let ty = (value.kind() == "composite_literal")
                .then(|| value.child_by_field_name("type"))
                .flatten();
            let annotation = ty.map(|node| self.type_text(node)).transpose()?;
            let spelling = self.bounded_text(text(name, self.source))?;
            (self.emit)(Fact::Binding(Binding {
                scope: context.scope,
                name: spelling,
                target: None,
                module: None,
                imported_name: None,
                type_name: annotation,
                kind: "local",
                span: span(name),
            }))?;
        }
        Ok(())
    }
}
