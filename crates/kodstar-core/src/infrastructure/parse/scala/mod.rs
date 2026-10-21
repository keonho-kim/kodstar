// Purpose: Scala given·타입 alias·generic scope·다중 매개변수와 타입 사용 추출.
// Pattern: Simple Module.
// Usage: 공통 선언·타입·매개변수 추출기가 Scala AST를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/parameters.rs, crates/kodstar-core/src/infrastructure/parse/type_uses.rs.

pub(super) mod bindings;

pub(super) mod imports;

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn scala_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language != Language::Scala {
            return Ok(None);
        }
        if let Some(value) = self.scala_value_definition(node)? {
            return Ok(Some(value));
        }
        if matches!(node.kind(), "type_definition" | "type_declaration") {
            return node
                .child_by_field_name("name")
                .map(|n| Ok((self.bounded_text(text(n, self.source))?, SymbolKind::Type)))
                .transpose();
        }
        if node.kind() == "given_definition" && node.child_by_field_name("name").is_none() {
            let ty = node
                .child_by_field_name("return_type")
                .map(|n| self.type_text(n))
                .transpose()?
                .unwrap_or_else(|| "<unknown-type>".into());
            let mut name = String::new();
            self.append_signature(&mut name, &["given ", &ty])?;
            return Ok(Some((name, SymbolKind::Class)));
        }
        Ok(None)
    }

    pub(in crate::infrastructure::parse) fn scala_type_parameters(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        let Some(parameters) = child_kind(node, &["type_parameters"]) else {
            return Ok(());
        };
        let mut cursor = parameters.walk();
        loop {
            let current = cursor.node();
            if matches!(current.kind(), "identifier" | "type_identifier")
                && cursor.field_name() == Some("name")
            {
                self.parameter_binding(current, None, "type_parameter", context)?;
            }
            if !matches!(
                current.kind(),
                "context_bound" | "upper_bound" | "lower_bound"
            ) && cursor.goto_first_child()
            {
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

    pub(in crate::infrastructure::parse) fn scala_parameters(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        let mut cursor = node.walk();
        for parameters in node
            .named_children(&mut cursor)
            .filter(|n| matches!(n.kind(), "parameters" | "class_parameters"))
        {
            self.parameter_list(parameters, context)?;
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn scala_bases(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if let Some(extend) = node.child_by_field_name("extend") {
            let mut cursor = extend.walk();
            for (index, ty) in extend
                .children_by_field_name("type", &mut cursor)
                .enumerate()
            {
                self.usage(
                    context,
                    self.reference_text(if ty.kind() == "generic_type" {
                        ty.child_by_field_name("type").unwrap_or(ty)
                    } else {
                        ty
                    })?,
                    if index == 0 {
                        RelationKind::Inherits
                    } else {
                        RelationKind::MixesIn
                    },
                    span(ty),
                )?;
            }
        }
        if node.kind() == "given_definition"
            && node
                .child_by_field_name("body")
                .is_some_and(|n| n.kind() == "with_template_body")
            && let Some(ty) = node.child_by_field_name("return_type")
        {
            let base = if ty.kind() == "generic_type" {
                ty.child_by_field_name("type").unwrap_or(ty)
            } else {
                ty
            };
            self.usage(
                context,
                self.reference_text(base)?,
                RelationKind::Implements,
                span(ty),
            )?;
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn scala_callable(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language == Language::Scala
            && let Some(ty) = node.child_by_field_name("return_type")
        {
            self.return_annotation(context, ty, self.type_text(ty)?)?;
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn scala_signature(
        &self,
        node: Node<'_>,
    ) -> Result<String> {
        let mut result = String::new();
        let mut cursor = node.walk();
        for list in node
            .named_children(&mut cursor)
            .filter(|n| matches!(n.kind(), "parameters" | "type_parameters"))
        {
            if list.kind() == "type_parameters" {
                self.append_signature(&mut result, &[&self.type_text(list)?])?;
                continue;
            }
            let contextual = {
                let mut cursor = list.walk();
                list.children(&mut cursor)
                    .any(|n| matches!(n.kind(), "using" | "implicit"))
            };
            self.append_signature(&mut result, &[if contextual { "(using " } else { "(" }])?;
            let mut parameters = list.walk();
            for parameter in list
                .named_children(&mut parameters)
                .filter(|n| n.kind() == "parameter")
            {
                let name = parameter
                    .child_by_field_name("name")
                    .map(|n| self.parameter_text(n))
                    .transpose()?
                    .unwrap_or_default();
                let ty = parameter
                    .child_by_field_name("type")
                    .map(|n| self.type_text(n))
                    .transpose()?
                    .unwrap_or_default();
                self.append_signature(&mut result, &[&name, ":", &ty, ","])?;
            }
            self.append_signature(&mut result, &[")"])?;
        }
        if let Some(ty) = node.child_by_field_name("return_type") {
            self.append_signature(&mut result, &[&self.type_text(ty)?])?;
        }
        Ok(result)
    }

    pub(in crate::infrastructure::parse) fn scala_type_use(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<bool> {
        if self.language != Language::Scala {
            return Ok(false);
        }
        if !matches!(
            node.kind(),
            "type_identifier" | "stable_type_identifier" | "projected_type"
        ) {
            return Ok(true);
        }
        let mut parent = node.parent();
        let mut prerequisite = false;
        while let Some(current) = parent {
            if matches!(current.kind(), "stable_type_identifier" | "projected_type") {
                return Ok(true);
            }
            if current.kind() == "self_type" {
                prerequisite = true;
                break;
            }
            if matches!(
                current.kind(),
                "class_definition"
                    | "trait_definition"
                    | "function_definition"
                    | "function_declaration"
                    | "type_definition"
                    | "given_definition"
            ) {
                break;
            }
            parent = current.parent();
        }
        let expression = if node.kind() == "projected_type" {
            self.bounded_text(text(node, self.source))?
        } else {
            self.reference_text(node)?
        };
        self.usage(
            context,
            expression,
            if prerequisite {
                RelationKind::DependsOn
            } else {
                RelationKind::UsesType
            },
            span(node),
        )?;
        Ok(true)
    }
}
