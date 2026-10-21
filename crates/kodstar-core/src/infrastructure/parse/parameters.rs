// Purpose: 언어별 매개변수·generic binding과 타입 추출.
// Pattern: Simple Module.
// Usage: 선언 추출기가 현재 노드와 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/infrastructure/parse/bindings.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn type_parameters(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        if self.language == Language::Scala {
            return self.scala_type_parameters(node, context);
        }
        if let Some(parameters) = node
            .child_by_field_name("type_parameters")
            .or_else(|| child_kind(node, &["type_parameters", "type_parameter_list"]))
        {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                let kind =
                    if self.language == Language::Rust && parameter.kind() == "const_parameter" {
                        "const_parameter"
                    } else {
                        "type_parameter"
                    };
                let mut names = parameter.walk();
                let mut found = false;
                for name in parameter.children_by_field_name("name", &mut names) {
                    self.parameter_binding(name, None, kind, context)?;
                    found = true;
                }
                if !found
                    && let Some(name) = child_kind(parameter, &["identifier", "type_identifier"])
                {
                    self.parameter_binding(name, None, kind, context)?;
                }
            }
        }
        if self.language == Language::Go
            && let Some(receiver) = node.child_by_field_name("receiver")
        {
            let mut cursor = receiver.walk();
            loop {
                let current = cursor.node();
                if current.kind() == "type_arguments" {
                    let mut args = current.walk();
                    for arg in current.named_children(&mut args) {
                        let name = if arg.kind() == "type_identifier" {
                            Some(arg)
                        } else {
                            child_kind(arg, &["type_identifier"])
                        };
                        if let Some(name) = name {
                            self.parameter_binding(name, None, "type_parameter", context)?;
                        }
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
        Ok(())
    }

    pub(super) fn parameters(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        if self.language == Language::CommonLisp {
            return self.lisp_parameters(node, context);
        }
        if let Some(type_name) = &context.type_name {
            let names: &[&str] = match self.language {
                Language::Python | Language::Rust | Language::Ruby | Language::Swift => &["self"],
                Language::Pascal => &["Self"],
                Language::JavaScript
                | Language::TypeScript
                | Language::Java
                | Language::CSharp
                | Language::Cpp
                | Language::Kotlin
                | Language::Scala
                | Language::Php => &["this"],
                _ => &[],
            };
            for name in names {
                (self.emit)(Fact::Binding(Binding {
                    scope: context.scope,
                    name: (*name).into(),
                    target: if matches!(self.language, Language::Kotlin | Language::Scala) {
                        context.type_owner
                    } else {
                        None
                    },
                    module: None,
                    imported_name: None,
                    type_name: if matches!(self.language, Language::Kotlin | Language::Scala)
                        && context.type_owner.is_some()
                    {
                        None
                    } else {
                        Some(type_name.render()?)
                    },
                    kind: if matches!(self.language, Language::Kotlin | Language::Scala)
                        && context.type_owner.is_some()
                    {
                        if self.language == Language::Scala {
                            "scala_self"
                        } else {
                            "kotlin_self"
                        }
                    } else {
                        "parameter"
                    },
                    span: span(node),
                }))?;
            }
        }
        if self.language == Language::Pascal {
            return self.pascal_parameters(node, context);
        }
        if self.language == Language::Scala {
            return self.scala_parameters(node, context);
        }
        for field in ["parameters", "parameter", "receiver", "args"] {
            if let Some(parameters) = node.child_by_field_name(field) {
                self.parameter_list(parameters, context)?;
            }
        }
        if matches!(self.language, Language::C | Language::Cpp)
            && let Some(parameters) =
                crate::infrastructure::parse::cxx::declarators::declarator(node)
                    .and_then(|n| n.child_by_field_name("parameters"))
        {
            self.parameter_list(parameters, context)?;
        }
        if node.child_by_field_name("parameters").is_none()
            && let Some(parameters) = child_kind(
                node,
                &[
                    "function_value_parameters",
                    "function_parameters",
                    "parameters",
                    "parameter_list",
                ],
            )
        {
            self.parameter_list(parameters, context)?;
        }
        Ok(())
    }

    pub(super) fn parameter_list(&mut self, parameters: Node<'_>, context: &Context) -> Result<()> {
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            let kind = if self.language == Language::Python
                && matches!(
                    parameter.kind(),
                    "default_parameter" | "typed_default_parameter"
                ) {
                "parameter_default"
            } else {
                "parameter"
            };
            let ty = parameter
                .child_by_field_name("type")
                .or_else(|| child_kind(parameter, &["type", "type_annotation", "user_type"]));
            let mut names = parameter.walk();
            let mut found = false;
            for name in parameter.children_by_field_name("name", &mut names) {
                self.parameter_binding(name, ty, kind, context)?;
                found = true;
            }
            if !found {
                let name = if matches!(
                    parameter.kind(),
                    "identifier" | "simple_identifier" | "variable_name"
                ) {
                    Some(parameter)
                } else {
                    parameter
                        .child_by_field_name("pattern")
                        .or_else(|| {
                            parameter
                                .child_by_field_name("declarator")
                                .and_then(crate::infrastructure::parse::syntax::name_node)
                        })
                        .or_else(|| {
                            child_kind(
                                parameter,
                                &["identifier", "simple_identifier", "variable_name"],
                            )
                        })
                };
                if let Some(name) = name {
                    self.parameter_binding(name, ty, kind, context)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn parameter_binding(
        &mut self,
        name: Node<'_>,
        ty: Option<Node<'_>>,
        kind: &'static str,
        context: &Context,
    ) -> Result<()> {
        let spelling = text(name, self.source).trim_start_matches('$');
        let inferred = if ty.is_none() && matches!(spelling, "self" | "this") {
            context
                .type_name
                .as_ref()
                .map(|name| name.render())
                .transpose()?
        } else {
            None
        };
        let annotation = ty
            .map(|node| self.type_text(node))
            .transpose()?
            .or(inferred);
        let spelling = self.bounded_text(spelling)?;
        (self.emit)(Fact::Binding(Binding {
            scope: context.scope,
            name: spelling,
            target: None,
            module: None,
            imported_name: None,
            type_name: annotation,
            kind,
            span: span(name),
        }))
    }
}
