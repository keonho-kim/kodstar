// Purpose: 언어별 타입 위치·decorator·attribute 사용 사실 추출.
// Pattern: Simple Module.
// Usage: 참조·선언 추출기가 원본 노드와 소유 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/references.rs, crates/kodstar-core/src/infrastructure/parse/declarations.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn type_use(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        let declared = context.declaration_name == Some((node.start_byte(), node.end_byte()))
            || node.parent().is_some_and(|p| {
                matches!(p.kind(), "type_parameter" | "type_parameter_declaration")
                    && p.child_by_field_name("name")
                        .is_some_and(|n| n.id() == node.id())
            });
        if declared {
            return Ok(());
        }
        if self.scala_type_use(node, context)? {
            return Ok(());
        }
        let qualified = matches!(
            node.kind(),
            "scoped_type_identifier" | "qualified_type" | "named_type" | "typeref" | "user_type"
        ) || node.kind() == "qualified_identifier"
            && crate::infrastructure::parse::cxx::declarators::leaf(node).kind()
                == "type_identifier";
        let part_of_type = node.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                "scoped_type_identifier"
                    | "qualified_type"
                    | "named_type"
                    | "typeref"
                    | "user_type"
            ) || p.kind() == "qualified_identifier"
                && crate::infrastructure::parse::cxx::declarators::leaf(p).kind()
                    == "type_identifier"
        });
        let identifier_type = node.kind() == "identifier"
            && node.parent().is_some_and(|p| {
                p.child_by_field_name("type")
                    .is_some_and(|n| n.id() == node.id())
                    || matches!(
                        (self.language, p.kind()),
                        (Language::Python, "type") | (Language::TypeScript, "type_annotation")
                    )
            });
        if !part_of_type && (qualified || node.kind() == "type_identifier" || identifier_type) {
            self.usage(
                context,
                self.reference_text(node)?,
                RelationKind::UsesType,
                span(node),
            )?;
        }
        if matches!(
            node.kind(),
            "annotation" | "marker_annotation" | "attribute"
        ) && matches!(
            self.language,
            Language::Java | Language::CSharp | Language::Kotlin
        ) && let Some(name) = node
            .child_by_field_name("name")
            .or_else(|| child_kind(node, &["identifier", "qualified_name", "user_type"]))
            .or_else(|| {
                child_kind(node, &["constructor_invocation"])
                    .and_then(|n| child_kind(n, &["user_type"]))
            })
        {
            self.usage(
                context,
                self.reference_text(name)?,
                RelationKind::References,
                span(name),
            )?;
        }
        if self.language == Language::Kotlin
            && node.kind() == "navigation_expression"
            && node
                .named_children(&mut node.walk())
                .last()
                .is_some_and(|n| text(n, self.source) == "class")
        {
            let mut cursor = node.walk();
            if node.children(&mut cursor).any(|n| n.kind() == "::")
                && let Some(receiver) = node.named_child(0)
            {
                self.usage(
                    context,
                    self.reference_text(receiver)?,
                    RelationKind::UsesType,
                    span(receiver),
                )?;
            }
        }
        Ok(())
    }
}
