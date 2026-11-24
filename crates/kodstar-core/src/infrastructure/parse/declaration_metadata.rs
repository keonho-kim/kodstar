// Purpose: 언어별 선언 이름과 제한된 서명·조건 메타데이터 추출.
// Pattern: Simple Module.
// Usage: 선언 추출과 Elixir 호출 판별이 현재 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/infrastructure/parse/references.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Extractor, child_kind, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn bounded_text(&self, value: &str) -> Result<String> {
        if value.len() > self.fact_limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "syntax text exceeds fact_batch_bytes",
            ));
        }
        Ok(value.to_owned())
    }

    pub(super) fn condition(&self, node: Node<'_>) -> Result<Option<String>> {
        if self.language == Language::Rust {
            let mut previous = node.prev_named_sibling();
            for _ in 0..16 {
                let Some(attribute) = previous else { break };
                if attribute.kind() != "attribute_item" {
                    break;
                }
                let value = text(attribute, self.source);
                if let Some(condition) =
                    crate::infrastructure::parse::spelling::conditional_attribute(value)
                {
                    return self.bounded_text(&condition).map(Some);
                }
                previous = attribute.prev_named_sibling();
            }
        }
        self.native_condition(node)
    }

    pub(super) fn signature(&self, node: Node<'_>) -> Result<String> {
        if self.language == Language::Scala {
            return self.scala_signature(node);
        }
        if self.language == Language::Swift {
            return self.swift_signature(node);
        }
        if self.language == Language::Kotlin {
            return self.kotlin_signature(node);
        }
        if self.language == Language::CommonLisp {
            return self.lisp_signature(node);
        }
        let parameters = node
            .child_by_field_name("parameters")
            .or_else(|| {
                crate::infrastructure::parse::cxx::declarators::declarator(node)
                    .and_then(|n| n.child_by_field_name("parameters"))
            })
            .or_else(|| child_kind(node, &["function_value_parameters", "parameter_list"]));
        let result = node
            .child_by_field_name("return_type")
            .or_else(|| node.child_by_field_name("result"))
            .or_else(|| node.child_by_field_name("type"));
        let mut text = String::from("(");
        if let Some(parameters) = parameters {
            let mut cursor = parameters.walk();
            for (index, parameter) in parameters.named_children(&mut cursor).enumerate() {
                let name = parameter
                    .child_by_field_name("name")
                    .or_else(|| parameter.child_by_field_name("pattern"))
                    .or_else(|| {
                        matches!(parameter.kind(), "identifier" | "self_parameter")
                            .then_some(parameter)
                    });
                let ty = parameter.child_by_field_name("type");
                let name = name
                    .map(|node| self.parameter_text(node))
                    .transpose()?
                    .unwrap_or_default();
                let ty = ty
                    .map(|node| self.type_text(node))
                    .transpose()?
                    .unwrap_or_default();
                self.append_signature(
                    &mut text,
                    &[if index == 0 { "" } else { "," }, &name, ":", &ty],
                )?;
            }
        }
        let result = result
            .map(|node| self.type_text(node))
            .transpose()?
            .unwrap_or_default();
        self.append_signature(&mut text, &[")", &result])?;
        Ok(text)
    }
    pub(super) fn append_signature(&self, text: &mut String, parts: &[&str]) -> Result<()> {
        let length = parts
            .iter()
            .fold(text.len(), |length, part| length.saturating_add(part.len()));
        if length > self.fact_limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "signature exceeds fact_batch_bytes",
            ));
        }
        text.try_reserve_exact(length - text.len())
            .map_err(|error| {
                let mut error = Error::caused_by("allocate signature", error);
                error.kind = ErrorKind::Limit;
                error
            })?;
        for part in parts {
            text.push_str(part);
        }
        Ok(())
    }
    pub(super) fn definition_exported(&self, node: Node<'_>, name: &str) -> bool {
        match self.language {
            Language::Swift => self.swift_exported(node),
            Language::Zig => {
                let declaration = if matches!(
                    node.kind(),
                    "struct_declaration"
                        | "enum_declaration"
                        | "union_declaration"
                        | "error_set_declaration"
                ) {
                    node.parent().unwrap_or(node)
                } else {
                    node
                };
                let mut cursor = declaration.walk();
                declaration.children(&mut cursor).any(|n| n.kind() == "pub")
            }
            Language::Go => name.chars().next().is_some_and(char::is_uppercase),
            Language::JavaScript | Language::TypeScript => {
                crate::infrastructure::parse::javascript::imports::exported(node)
            }
            Language::Elixir => !node.child_by_field_name("target").is_some_and(|n| {
                matches!(text(n, self.source), "defp" | "defmacrop" | "defguardp")
            }),
            _ => true,
        }
    }
}
