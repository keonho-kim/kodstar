// Purpose: Ruby implicit 호출·정적 mixin과 lookup 변경 근거 추출.
// Pattern: Simple Module.
// Usage: 참조 순회가 Ruby 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/references.rs, crates/kodstar-core/src/infrastructure/parse/ruby/mod.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Fact, SyntaxUse};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::ruby::BarrierScope;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn ruby_facts(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<bool> {
        if self.language != Language::Ruby {
            return Ok(false);
        }
        if matches!(node.kind(), "alias" | "undef") {
            self.ruby_barrier(c, "method alias/removal", BarrierScope::Owner, span(node))?;
        }
        if node.kind() == "identifier"
            && node
                .parent()
                .is_some_and(|p| matches!(p.kind(), "body_statement" | "then" | "else"))
        {
            if matches!(text(node, self.source), "private" | "protected" | "public") {
                self.ruby_barrier(
                    c,
                    "method visibility mutation",
                    BarrierScope::Owner,
                    span(node),
                )?;
                return Ok(true);
            }
            self.usage(
                c,
                self.reference_text(node)?,
                RelationKind::Calls,
                span(node),
            )?;
        }
        if node.kind() != "call" {
            return Ok(false);
        }
        if node
            .parent()
            .is_some_and(|p| crate::infrastructure::parse::ruby::factory(p, self.source))
        {
            return Ok(true);
        }
        let Some(method) = node.child_by_field_name("method") else {
            return Ok(false);
        };
        let method = text(method, self.source);
        let implicit = node
            .child_by_field_name("receiver")
            .is_none_or(|n| n.kind() == "self");
        if matches!(method, "refine" | "using") {
            self.ruby_barrier(c, "refinement activation", BarrierScope::File, span(node))?;
        }
        if implicit
            && matches!(
                method,
                "alias_method"
                    | "attr"
                    | "attr_accessor"
                    | "attr_reader"
                    | "attr_writer"
                    | "define_method"
                    | "define_singleton_method"
                    | "include"
                    | "extend"
                    | "module_function"
                    | "prepend"
                    | "remove_method"
                    | "undef_method"
            )
        {
            self.ruby_barrier(c, method, BarrierScope::Owner, span(node))?;
        }
        if !implicit
            && matches!(
                method,
                "class_eval"
                    | "class_exec"
                    | "module_eval"
                    | "module_exec"
                    | "define_method"
                    | "define_singleton_method"
                    | "include"
                    | "extend"
                    | "prepend"
                    | "alias_method"
                    | "remove_method"
                    | "undef_method"
            )
        {
            self.ruby_barrier(
                c,
                "external owner mutation",
                BarrierScope::Project,
                span(node),
            )?;
        }
        if implicit && matches!(method, "include" | "extend" | "prepend") {
            if let Some(args) = node.child_by_field_name("arguments") {
                let mut cursor = args.walk();
                for name in args
                    .named_children(&mut cursor)
                    .filter(|n| matches!(n.kind(), "constant" | "scope_resolution"))
                {
                    let expression = format!("@ruby-mixin:{method}#{}", self.reference_text(name)?);
                    (self.emit)(Fact::Use(SyntaxUse {
                        source: c.type_owner.unwrap_or(c.source),
                        scope: c.scope,
                        expression,
                        relation: RelationKind::MixesIn,
                        span: span(name),
                        condition: c.condition.as_deref().map(str::to_owned),
                        type_only: false,
                    }))?;
                }
            }
            return Ok(true);
        }
        Ok(false)
    }
}
