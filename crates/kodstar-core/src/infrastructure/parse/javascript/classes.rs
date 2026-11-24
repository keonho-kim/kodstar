// Purpose: JS/TS 클래스 표현식의 지역 이름과 직접 대입·export 별칭 추출.
// Pattern: Simple Module.
// Usage: definition 추출기가 클래스 scope를 만든 뒤 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs, crates/kodstar-core/src/infrastructure/parse/javascript/commonjs.rs.

use crate::domain::{
    diagnostics::Result,
    facts::{Binding, Fact},
    languages::Language,
};
use crate::infrastructure::parse::{Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn class_expression(&self, node: Node<'_>) -> bool {
        matches!(self.language, Language::JavaScript | Language::TypeScript)
            && node.kind() == "class"
    }

    pub(in crate::infrastructure::parse) fn class_assignment(
        &mut self,
        node: Node<'_>,
        target: u32,
        scope: u32,
    ) -> Result<()> {
        let Some(parent) = node.parent() else {
            return Ok(());
        };
        let assigned = match parent.kind() {
            "variable_declarator" => parent.child_by_field_name("name"),
            "assignment_expression" => parent.child_by_field_name("left"),
            _ => None,
        };
        if let Some(assigned) = assigned {
            let spelling = text(assigned, self.source);
            if assigned.kind() == "identifier" || spelling == "module.exports" {
                let name = self.bounded_text(spelling)?;
                (self.emit)(Fact::Binding(Binding {
                    scope,
                    name,
                    target: Some(target),
                    module: None,
                    imported_name: None,
                    type_name: None,
                    kind: "type",
                    span: span(node),
                }))?;
            }
        }
        Ok(())
    }
}
