// Purpose: Erlang 정적 type·opaque·macro 선언과 타입 문맥 분류.
// Pattern: Simple Module.
// Usage: 특수 선언·호출 추출기가 사용하며 전처리를 실행하지 않음.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs, crates/kodstar-core/src/infrastructure/parse/references.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Extractor, text};
use tree_sitter::Node;

pub(in crate::infrastructure::parse) fn type_context(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "type_alias" | "opaque" | "spec" | "callback" | "field_type"
        ) {
            return true;
        }
        if matches!(parent.kind(), "fun_decl" | "pp_define") {
            return false;
        }
        node = parent;
    }
    false
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn erlang_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language != Language::Erlang {
            return Ok(None);
        }
        let field = match node.kind() {
            "type_alias" | "opaque" => "name",
            "pp_define" => "lhs",
            _ => return Ok(None),
        };
        let Some(container) = node.child_by_field_name(field) else {
            return Ok(None);
        };
        let Some(name) = container.child_by_field_name("name") else {
            return Ok(None);
        };
        let mut label = self.bounded_text(text(name, self.source).trim_matches('\''))?;
        let args = container.child_by_field_name("args");
        if node.kind() != "pp_define" || args.is_some() {
            self.append_signature(
                &mut label,
                &["/", &args.map_or(0, |n| n.named_child_count()).to_string()],
            )?;
        }
        Ok(Some((
            label,
            if node.kind() == "pp_define" {
                SymbolKind::Macro
            } else {
                SymbolKind::Type
            },
        )))
    }
}
