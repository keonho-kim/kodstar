// Purpose: JSX/TSX component 태그의 명시적 값 참조 추출.
// Pattern: Simple Module.
// Usage: 공통 참조 walker가 각 opening/self-closing 태그에서 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/references.rs, crates/kodstar-core/src/application/analysis/resolve/modules/exports.rs.

pub(super) mod commonjs;
pub(super) mod ejs;
pub(super) mod imports;

pub(super) mod decorators;

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn jsx_reference(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if !matches!(self.language, Language::JavaScript | Language::TypeScript)
            || !matches!(
                node.kind(),
                "jsx_opening_element" | "jsx_self_closing_element"
            )
        {
            return Ok(());
        }
        let Some(name) = node.child_by_field_name("name") else {
            return Ok(());
        };
        let spelling = text(name, self.source);
        if spelling.chars().next().is_some_and(char::is_uppercase) || spelling.contains('.') {
            self.usage(
                context,
                self.reference_text(name)?,
                RelationKind::References,
                span(name),
            )?;
        }
        Ok(())
    }
}
