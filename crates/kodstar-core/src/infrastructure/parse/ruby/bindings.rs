// Purpose: Ruby block scope·매개변수와 생성 결과 지역 binding 추출.
// Pattern: Simple Module.
// Usage: Ruby 문맥 진입과 일반 변수 추출기가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/ruby/mod.rs, crates/kodstar-core/src/infrastructure/parse/bindings.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact, Scope};
use crate::domain::languages::Language;
use crate::infrastructure::parse::ruby::BarrierScope;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn ruby_block(
        &mut self,
        node: Node<'_>,
        c: &mut Context,
    ) -> Result<()> {
        if !matches!(node.kind(), "do_block" | "block") {
            return Ok(());
        }
        let id = self.allocate()?;
        (self.emit)(Fact::Scope(Scope {
            local_id: id,
            parent: Some(c.scope),
            span: span(node),
        }))?;
        c.scope = id;
        if let Some(parameters) = node.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for name in parameters
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "identifier")
            {
                let name_text = self.bounded_text(text(name, self.source))?;
                (self.emit)(Fact::Binding(Binding {
                    scope: id,
                    name: name_text,
                    target: None,
                    module: None,
                    imported_name: None,
                    type_name: None,
                    kind: "parameter",
                    span: span(name),
                }))?;
            }
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn ruby_binding(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<bool> {
        if self.language != Language::Ruby
            || node.kind() != "assignment"
            || crate::infrastructure::parse::ruby::factory(node, self.source)
        {
            return Ok(false);
        }
        let (Some(name), Some(value)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return Ok(false);
        };
        if !matches!(name.kind(), "identifier" | "constant") {
            return Ok(false);
        }
        let producer = if value.kind() == "call"
            && value
                .child_by_field_name("method")
                .is_some_and(|n| text(n, self.source) == "new")
            && value
                .child_by_field_name("receiver")
                .is_some_and(|n| matches!(n.kind(), "constant" | "scope_resolution"))
        {
            Some(self.call_text(value, self.callee(value).unwrap_or(value))?)
        } else {
            None
        };
        let name_text = self.bounded_text(text(name, self.source))?;
        (self.emit)(Fact::Binding(Binding {
            scope: c.scope,
            name: name_text,
            target: None,
            module: None,
            imported_name: producer.clone(),
            type_name: None,
            kind: if producer.is_some() {
                "call_result"
            } else {
                "local"
            },
            span: span(name),
        }))?;
        if name.kind() == "constant" {
            self.ruby_barrier(c, "constant rebinding", BarrierScope::Project, span(node))?;
        }
        Ok(true)
    }
}
