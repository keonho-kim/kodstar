// Purpose: 언어별 명시적 상속·interface·trait 사용 사실 추출.
// Pattern: Simple Module.
// Usage: 선언 추출기가 현재 소유 문맥과 AST를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/infrastructure/parse/references.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn inheritance(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        if self.language == Language::Scala {
            return self.scala_bases(node, context);
        }
        for (field, relation) in [
            ("superclasses", RelationKind::Inherits),
            ("superclass", RelationKind::Inherits),
            ("interfaces", RelationKind::Implements),
            ("trait", RelationKind::Implements),
            ("extend", RelationKind::Inherits),
        ] {
            if let Some(base) = node.child_by_field_name(field) {
                self.base_types(base, context, relation)?;
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if self.language == Language::Kotlin && child.kind() == "delegation_specifiers" {
                self.kotlin_bases(child, context)?;
                continue;
            }
            let relation = match child.kind() {
                "class_interface_clause" | "super_interfaces" => RelationKind::Implements,
                "base_list"
                | "base_class_clause"
                | "base_clause"
                | "inheritance_specifier"
                | "delegation_specifiers" => RelationKind::Inherits,
                _ => continue,
            };
            self.base_types(child, context, relation)?;
        }
        if self.language == Language::CommonLisp
            && node.kind() == "list_lit"
            && node
                .named_child(0)
                .is_some_and(|n| text(n, self.source).eq_ignore_ascii_case("defclass"))
            && let Some(bases) = node.named_child(2)
        {
            self.base_types(bases, context, RelationKind::Inherits)?;
        }
        if self.language == Language::Ruby
            && node.kind() == "assignment"
            && let Some(right) = node.child_by_field_name("right")
            && right
                .child_by_field_name("receiver")
                .is_some_and(|n| text(n, self.source) == "Class")
            && let Some(args) = right
                .child_by_field_name("arguments")
                .and_then(|n| n.named_child(0))
        {
            self.base_types(args, context, RelationKind::Inherits)?;
        }
        Ok(())
    }

    fn base_types(
        &mut self,
        root: Node<'_>,
        context: &Context,
        relation: RelationKind,
    ) -> Result<()> {
        let mut cursor = root.walk();
        loop {
            let node = cursor.node();
            let named_type = matches!(
                node.kind(),
                "identifier"
                    | "type_identifier"
                    | "generic_type"
                    | "scoped_type_identifier"
                    | "user_type"
                    | "generic_name"
                    | "qualified_name"
                    | "namespace_name"
                    | "name"
                    | "constant"
                    | "scope_resolution"
                    | "sym_lit"
            );
            if named_type {
                self.usage(context, self.reference_text(node)?, relation, span(node))?;
            }
            if !named_type
                && (node.id() == root.id()
                    || !matches!(
                        node.kind(),
                        "arguments" | "argument_list" | "value_arguments" | "type_arguments"
                    ))
                && cursor.goto_first_child()
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
}
