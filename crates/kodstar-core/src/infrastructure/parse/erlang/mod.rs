// Purpose: Erlang export·behaviour·record 타입·함수 값과 include 사실 추출.
// Pattern: Simple Module.
// Usage: 구문 참조 순회가 Erlang 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/references.rs, crates/kodstar-core/src/application/analysis/resolve/identities.rs.

pub(super) mod types;

pub(super) mod declarations;
pub(super) mod imports;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn erlang_facts(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Erlang {
            return Ok(());
        }
        match node.kind() {
            "fa" if node.parent().is_some_and(|p| {
                matches!(p.kind(), "export_attribute" | "export_type_attribute")
            }) =>
            {
                if let Some(name) = function_arity(node, self.source) {
                    (self.emit)(Fact::Binding(Binding {
                        scope: 0,
                        name,
                        target: None,
                        module: None,
                        imported_name: None,
                        type_name: None,
                        kind: if node
                            .parent()
                            .is_some_and(|p| p.kind() == "export_type_attribute")
                        {
                            "erlang_type_export"
                        } else {
                            "erlang_export"
                        },
                        span: span(node),
                    }))?;
                }
            }
            "compile_options_attribute" => {
                let mut cursor = node.walk();
                loop {
                    let n = cursor.node();
                    if n.kind() == "atom" && text(n, self.source) == "export_all" {
                        (self.emit)(Fact::Binding(Binding {
                            scope: 0,
                            name: "*".into(),
                            target: None,
                            module: None,
                            imported_name: None,
                            type_name: None,
                            kind: "erlang_export",
                            span: span(n),
                        }))?;
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
            "internal_fun" | "external_fun" => {
                if let Some(name) = function_arity(node, self.source) {
                    let expression = if let Some(module) = node
                        .child_by_field_name("module")
                        .and_then(|n| n.child_by_field_name("name"))
                    {
                        if module.kind() != "atom" {
                            return Ok(());
                        }
                        format!(
                            "@module:{}#{name}",
                            text(module, self.source).trim_matches('\'')
                        )
                    } else {
                        name
                    };
                    self.named_callable_value(node, context, "erlang_fun", expression)?;
                }
            }
            "macro_call_expr" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let mut expression =
                        self.bounded_text(text(name, self.source).trim_matches('\''))?;
                    if let Some(args) = node.child_by_field_name("args") {
                        self.append_signature(
                            &mut expression,
                            &["/", &args.named_child_count().to_string()],
                        )?;
                    }
                    self.append_signature(&mut expression, &["!"])?;
                    self.usage(context, expression, RelationKind::References, span(name))?;
                }
            }
            "record_expr" | "record_field_expr" | "record_index_expr" => {
                if let Some(name) = node
                    .child_by_field_name("name")
                    .and_then(|n| n.child_by_field_name("name"))
                {
                    self.usage(
                        context,
                        self.bounded_text(text(name, self.source).trim_matches('\''))?,
                        RelationKind::UsesType,
                        span(name),
                    )?;
                }
            }
            "behaviour_attribute" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.usage(
                        context,
                        format!("@module:{}", text(name, self.source).trim_matches('\'')),
                        RelationKind::Implements,
                        span(name),
                    )?;
                }
            }
            "pp_include" | "pp_include_lib" => {
                if let Some(file) = node.child_by_field_name("file") {
                    let raw = text(file, self.source).trim_matches('"');
                    let expression = if node.kind() == "pp_include" {
                        format!("@file:{raw}")
                    } else {
                        format!("@package:erlang-include:{raw}")
                    };
                    self.usage(context, expression, RelationKind::Includes, span(file))?;
                    if node.kind() == "pp_include" {
                        (self.emit)(Fact::Binding(Binding {
                            scope: 0,
                            name: "*".into(),
                            target: None,
                            module: Some(format!("@erlang-include:{raw}")),
                            imported_name: Some("*".into()),
                            type_name: None,
                            kind: "include",
                            span: span(file),
                        }))?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub(in crate::infrastructure::parse) fn function_arity(
    node: Node<'_>,
    source: &[u8],
) -> Option<String> {
    let function = node.child_by_field_name("fun")?;
    if function.kind() != "atom" {
        return None;
    }
    let arity = node
        .child_by_field_name("arity")?
        .child_by_field_name("value")?;
    let arity = text(arity, source).parse::<u32>().ok()?;
    Some(format!(
        "{}/{arity}",
        text(function, source).trim_matches('\'')
    ))
}
