// Purpose: 전처리 구조 매크로의 미확정 사용과 이름 문맥 경계를 보존.
// Pattern: Syntax Adapter.
// Usage: C/C++ 순회가 macro block·declaration을 발견하면 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/native_macros.rs, crates/kodstar-core/src/infrastructure/parse/forms.rs.

use super::super::{Context, Extractor, span, text};
use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::relations::RelationKind;
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn cxx_macro(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if node.kind() == "preproc_function_def" {
            if let Some(name) = node.child_by_field_name("name")
                && text(name, self.source)
                    .bytes()
                    .any(|byte| byte.is_ascii_lowercase())
            {
                self.native_macro_binding(name, "native_macro_definition")?;
            }
            return Ok(());
        }
        let name = if node.kind() == "type_qualifier"
            && node
                .named_child(0)
                .is_none_or(|child| child.kind() != "type_qualifier")
            && text(node, self.source)
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            Some(node)
        } else if matches!(
            node.kind(),
            "macro_block" | "macro_declaration" | "macro_statement" | "macro_type"
        ) {
            node.child_by_field_name("function")
                .or_else(|| node.child_by_field_name("name"))
        } else {
            None
        };
        let Some(name) = name else {
            return Ok(());
        };
        if text(name, self.source)
            .bytes()
            .any(|byte| byte.is_ascii_lowercase())
        {
            self.native_macro_binding(name, "native_macro_requirement")?;
        }
        let expression =
            self.bounded_text(&format!("@native-macro:{}", text(name, self.source)))?;
        self.usage(context, expression, RelationKind::References, span(name))?;
        if !self.native_macro_context {
            self.native_macro_context = true;
            (self.emit)(Fact::Binding(Binding {
                scope: 0,
                name: "<native-macro-context>".into(),
                target: None,
                module: None,
                imported_name: None,
                type_name: None,
                kind: "native_macro_context",
                span: span(name),
            }))?;
        }
        Ok(())
    }

    fn native_macro_binding(&mut self, name: Node<'_>, kind: &'static str) -> Result<()> {
        let spelling = self.bounded_text(&format!("@native-macro:{}", text(name, self.source)))?;
        (self.emit)(Fact::Binding(Binding {
            scope: 0,
            name: spelling,
            target: None,
            module: None,
            imported_name: None,
            type_name: None,
            kind,
            span: span(name),
        }))
    }
}
