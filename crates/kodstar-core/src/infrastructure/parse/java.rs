// Purpose: Java AST의 선언 인자 범위와 호출 인자 수 추출.
// Pattern: Simple Module.
// Usage: 정의·호출 추출기가 Java method 노드에서 호출.
// Related: crates/kodstar-core/src/domain/resolve/java.rs, crates/kodstar-core/src/infrastructure/parse/call_expressions.rs.

use crate::domain::{
    diagnostics::Result,
    facts::{Binding, Fact},
    languages::Language,
    resolve::java::{CALL, PARAMETERS},
};
use crate::infrastructure::parse::{Context, Extractor, span};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn java_method_parameters(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Java || node.kind() != "method_declaration" {
            return Ok(());
        }
        let Some(parameters) = node
            .child_by_field_name("parameters")
            .filter(|node| !node.has_error())
        else {
            return Ok(());
        };
        let mut cursor = parameters.walk();
        let mut minimum = 0usize;
        let mut variadic = false;
        for parameter in parameters.named_children(&mut cursor) {
            match parameter.kind() {
                "formal_parameter" => minimum += 1,
                "spread_parameter" => variadic = true,
                _ => {}
            }
        }
        (self.emit)(Fact::Binding(Binding {
            scope: context.scope,
            name: "@java_arity".into(),
            target: Some(context.source),
            module: None,
            imported_name: Some(minimum.to_string()),
            type_name: Some(if variadic { "variadic" } else { "fixed" }.into()),
            kind: PARAMETERS,
            span: span(parameters),
        }))
    }

    pub(super) fn java_call(&self, node: Node<'_>, spelling: String) -> Result<String> {
        let Some(arguments) = node
            .child_by_field_name("arguments")
            .filter(|args| !args.has_error())
        else {
            return Ok(spelling);
        };
        let mut cursor = arguments.walk();
        let count = arguments
            .named_children(&mut cursor)
            .filter(|child| !child.is_extra())
            .count();
        let mut expression = String::new();
        self.append_signature(&mut expression, &[CALL, &count.to_string(), "#", &spelling])?;
        Ok(expression)
    }
}
