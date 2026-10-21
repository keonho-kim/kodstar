// Purpose: Python comprehension의 내부 scope와 바깥 iterable 평가 문맥 분리.
// Pattern: Simple Module.
// Usage: AST 노드 진입이 현재 추출 문맥을 갱신하며 binding은 walrus scope를 선택.
// Related: crates/kodstar-core/src/infrastructure/parse/context.rs, crates/kodstar-core/src/infrastructure/parse/python/mod.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Fact, Scope};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span};
use tree_sitter::Node;

#[derive(Clone, Copy)]
pub(in crate::infrastructure::parse) struct Comprehension {
    scope: u32,
    member_scope: bool,
    start: usize,
    end: usize,
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn python_comprehension(
        &mut self,
        node: Node<'_>,
        context: &mut Context,
    ) -> Result<()> {
        if self.language != Language::Python {
            return Ok(());
        }
        if let Some(outer) = context.python_comprehension
            && node.start_byte() == outer.start
            && node.end_byte() == outer.end
        {
            context.scope = outer.scope;
            context.member_scope = outer.member_scope;
            context.python_comprehension = None;
        }
        if !matches!(
            node.kind(),
            "list_comprehension"
                | "set_comprehension"
                | "dictionary_comprehension"
                | "generator_expression"
        ) {
            return Ok(());
        }
        let Some(iterable) =
            child_kind(node, &["for_in_clause"]).and_then(|n| n.child_by_field_name("right"))
        else {
            return Ok(());
        };
        let outer = Comprehension {
            scope: context.scope,
            member_scope: context.member_scope,
            start: iterable.start_byte(),
            end: iterable.end_byte(),
        };
        let scope = self.allocate()?;
        (self.emit)(Fact::Scope(Scope {
            local_id: scope,
            parent: Some(if context.member_scope {
                context.member_outer
            } else {
                context.scope
            }),
            span: span(node),
        }))?;
        context.scope = scope;
        context.member_scope = false;
        context.python_comprehension = Some(outer);
        Ok(())
    }
}
