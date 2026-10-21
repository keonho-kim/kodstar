// Purpose: 언어별 callee·호출 결과 표현과 재귀 깊이 제한.
// Pattern: Simple Module.
// Usage: 참조·반환/지역 binding 추출기가 호출 표현을 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/references.rs, crates/kodstar-core/src/infrastructure/parse/return_values.rs, crates/kodstar-core/src/infrastructure/parse/spelling.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Extractor, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn call_text(&self, node: Node<'_>, callee: Node<'_>) -> Result<String> {
        self.call_text_at(node, callee, 0)
    }

    fn call_text_at(&self, node: Node<'_>, callee: Node<'_>, depth: usize) -> Result<String> {
        if depth >= 32 {
            return Err(crate::domain::diagnostics::Error::new(
                crate::domain::diagnostics::ErrorKind::Limit,
                "call result chain exceeds 32 levels",
            ));
        }
        if self.language == Language::Scala && node.kind() == "instance_expression" {
            let mut result = String::new();
            self.append_signature(
                &mut result,
                &["@constructor:", &self.reference_text(callee)?],
            )?;
            return Ok(result);
        }
        if self.language == Language::Swift
            && callee.kind() == "navigation_expression"
            && let (Some(receiver), Some(suffix)) = (
                callee.child_by_field_name("target"),
                callee.child_by_field_name("suffix"),
            )
            && receiver.kind() == "call_expression"
            && let Some(member) = suffix.child_by_field_name("suffix")
        {
            let producer = self.call_text_at(
                receiver,
                self.callee(receiver).unwrap_or(receiver),
                depth + 1,
            )?;
            let member = self.reference_text(member)?;
            let mut result = String::new();
            self.append_signature(&mut result, &["@result:", &producer, "#", &member])?;
            return Ok(result);
        }
        if self.language == Language::Kotlin
            && callee.kind() == "navigation_expression"
            && let (Some(receiver), Some(member)) = (callee.named_child(0), callee.named_child(1))
            && receiver.kind() == "call_expression"
        {
            let producer = self.call_text_at(
                receiver,
                self.callee(receiver).unwrap_or(receiver),
                depth + 1,
            )?;
            let member = self.reference_text(member)?;
            let mut result = String::new();
            self.append_signature(&mut result, &["@result:", &producer, "#", &member])?;
            return Ok(result);
        }
        if self.language == Language::Ruby
            && node.kind() == "call"
            && node
                .child_by_field_name("method")
                .is_some_and(|n| text(n, self.source) == "new")
            && let Some(owner) = node
                .child_by_field_name("receiver")
                .filter(|n| matches!(n.kind(), "constant" | "scope_resolution"))
        {
            let mut result = String::new();
            self.append_signature(
                &mut result,
                &["@constructor:", &self.reference_text(owner)?],
            )?;
            return Ok(result);
        }
        if matches!(self.language, Language::Java | Language::CSharp) {
            if node.kind() == "object_creation_expression" {
                let ty = node.child_by_field_name("type").unwrap_or(callee);
                let mut result = String::new();
                self.append_signature(&mut result, &["@constructor:", &self.reference_text(ty)?])?;
                return Ok(result);
            }
            let parts = if self.language == Language::Java {
                node.child_by_field_name("object")
                    .zip(node.child_by_field_name("name"))
            } else if callee.kind() == "member_access_expression" {
                callee
                    .child_by_field_name("expression")
                    .zip(callee.child_by_field_name("name"))
            } else {
                None
            };
            if let Some((receiver, member)) =
                parts.filter(|(r, _)| self.config.calls.contains(&r.kind()))
            {
                let producer = self.call_text_at(
                    receiver,
                    self.callee(receiver).unwrap_or(receiver),
                    depth + 1,
                )?;
                let member = self.reference_text(member)?;
                let mut result = String::new();
                self.append_signature(&mut result, &["@result:", &producer, "#", &member])?;
                return Ok(result);
            }
        }
        if self.language == Language::Php {
            if node.kind() == "function_call_expression" && callee.kind() == "variable_name" {
                return Ok("<dynamic>()".into());
            }
            if node.kind() == "object_creation_expression" {
                let name = self.reference_text(callee)?;
                let mut result = String::new();
                self.append_signature(&mut result, &["@php-new:", &name])?;
                return Ok(result);
            }
            if node.kind() == "scoped_call_expression"
                && let (Some(scope), Some(name)) = (
                    node.child_by_field_name("scope"),
                    node.child_by_field_name("name"),
                )
            {
                let mut result = String::new();
                self.append_signature(
                    &mut result,
                    &[
                        &self.reference_text(scope)?,
                        "::",
                        &self.reference_text(name)?,
                    ],
                )?;
                return Ok(result);
            }
        }
        if self.language == Language::Python
            && callee.kind() == "attribute"
            && let Some(receiver) = callee
                .child_by_field_name("object")
                .filter(|n| n.kind() == "call")
            && let (Some(producer), Some(member)) = (
                receiver.child_by_field_name("function"),
                callee.child_by_field_name("attribute"),
            )
        {
            let producer = self.call_text_at(receiver, producer, depth + 1)?;
            let member = self.reference_text(member)?;
            if producer == "super"
                && receiver
                    .child_by_field_name("arguments")
                    .is_some_and(|n| n.named_child_count() == 0)
            {
                return Ok(format!("@super:{member}"));
            }
            let mut value = String::new();
            self.append_signature(&mut value, &["@result:", &producer, "#", &member])?;
            return Ok(value);
        }
        if self.language == Language::Php
            && matches!(
                node.kind(),
                "member_call_expression" | "nullsafe_member_call_expression"
            )
            && let (Some(object), Some(name)) = (
                node.child_by_field_name("object"),
                node.child_by_field_name("name"),
            )
        {
            let receiver = self.reference_text(object)?;
            let name = self.reference_text(name)?;
            let mut expression = String::new();
            if matches!(
                object.kind(),
                "function_call_expression"
                    | "member_call_expression"
                    | "nullsafe_member_call_expression"
                    | "scoped_call_expression"
                    | "object_creation_expression"
            ) {
                let callee = object
                    .child_by_field_name("function")
                    .or_else(|| object.named_child(0))
                    .unwrap_or(object);
                let producer = self.call_text_at(object, callee, depth + 1)?;
                self.append_signature(&mut expression, &["@result:", &producer, "#", &name])?;
                return Ok(expression);
            }
            self.append_signature(&mut expression, &[&receiver, "::", &name])?;
            return Ok(expression);
        }
        let name = self.reference_text(callee)?;
        if matches!(self.config.call_field, "name" | "method")
            && let Some(object) = node
                .child_by_field_name("object")
                .or_else(|| node.child_by_field_name("receiver"))
        {
            let receiver = self.reference_text(object)?;
            let mut expression = String::new();
            self.append_signature(&mut expression, &[&receiver, ".", &name])?;
            return Ok(expression);
        }
        Ok(name)
    }
}
