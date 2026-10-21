// Purpose: Ruby 정적 class factory와 메서드 dispatch·lookup barrier 사실 추출.
// Pattern: Simple Module.
// Usage: 선언 순회가 Ruby 문맥과 정의 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs, crates/kodstar-core/src/application/analysis/resolve/ruby/mod.rs.

pub(super) mod bindings;
pub(super) mod imports;
pub(super) mod references;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

pub(in crate::infrastructure::parse) enum BarrierScope {
    Owner,
    File,
    Project,
}

pub(in crate::infrastructure::parse) fn factory(node: Node<'_>, source: &[u8]) -> bool {
    if node.kind() != "assignment"
        || !node
            .child_by_field_name("left")
            .is_some_and(|n| n.kind() == "constant")
    {
        return false;
    }
    let Some(call) = node
        .child_by_field_name("right")
        .filter(|n| n.kind() == "call")
    else {
        return false;
    };
    let (Some(receiver), Some(method)) = (
        call.child_by_field_name("receiver"),
        call.child_by_field_name("method"),
    ) else {
        return false;
    };
    receiver.kind() == "constant"
        && matches!(
            (text(receiver, source), text(method, source)),
            ("Class" | "Struct", "new") | ("Data", "define")
        )
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn ruby_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language == Language::Ruby
            && factory(node, self.source)
            && let Some(name) = node.child_by_field_name("left")
        {
            return Ok(Some((
                self.bounded_text(text(name, self.source))?,
                SymbolKind::Class,
            )));
        }
        Ok(None)
    }

    pub(in crate::infrastructure::parse) fn ruby_context(
        &mut self,
        node: Node<'_>,
        c: &mut Context,
    ) -> Result<()> {
        if self.language != Language::Ruby {
            return Ok(());
        }
        if node.kind() == "singleton_class" {
            c.ruby_singleton_depth = c.ruby_singleton_depth.saturating_add(1);
            if !node
                .child_by_field_name("value")
                .is_some_and(|n| n.kind() == "self")
            {
                c.ruby_singleton_depth = 2;
                self.ruby_barrier(
                    c,
                    "dynamic singleton owner",
                    BarrierScope::Project,
                    span(node),
                )?;
            }
        }
        self.ruby_block(node, c)
    }

    pub(in crate::infrastructure::parse) fn ruby_metadata(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if self.language != Language::Ruby {
            return Ok(());
        }
        if factory(node, self.source) {
            self.ruby_barrier(c, "class factory", BarrierScope::Owner, span(node))?;
        }
        if matches!(node.kind(), "method" | "singleton_method") {
            let mode = if node.kind() == "singleton_method" {
                if c.ruby_singleton_depth == 0
                    && node
                        .child_by_field_name("object")
                        .is_some_and(|n| n.kind() == "self")
                {
                    "singleton"
                } else {
                    "ambiguous"
                }
            } else {
                match c.ruby_singleton_depth {
                    0 => "instance",
                    1 => "singleton",
                    _ => "ambiguous",
                }
            };
            if c.type_owner.is_some()
                || node.kind() == "singleton_method"
                || c.ruby_singleton_depth > 0
            {
                (self.emit)(Fact::Binding(Binding {
                    scope: c.scope,
                    name: "@ruby-dispatch".into(),
                    target: Some(c.source),
                    module: Some(mode.into()),
                    imported_name: None,
                    type_name: None,
                    kind: "ruby_dispatch",
                    span: span(node),
                }))?;
            }
            let direct = node.parent().is_some_and(|p| {
                p.kind() == "program"
                    || p.kind() == "body_statement"
                        && p.parent().is_some_and(|n| {
                            matches!(
                                n.kind(),
                                "class" | "module" | "singleton_class" | "do_block" | "block"
                            )
                        })
            });
            if !direct {
                self.ruby_barrier(
                    c,
                    "conditional or nested method definition",
                    BarrierScope::Owner,
                    span(node),
                )?;
            }
            if let Some(object) = node
                .child_by_field_name("object")
                .filter(|n| matches!(n.kind(), "constant" | "scope_resolution"))
            {
                let owner = self.reference_text(object)?;
                (self.emit)(Fact::Binding(Binding {
                    scope: c.scope,
                    name: "@owner".into(),
                    target: Some(c.source),
                    module: None,
                    imported_name: Some(owner.clone()),
                    type_name: None,
                    kind: "owner_hint",
                    span: span(object),
                }))?;
                if c.type_owner.is_none() {
                    (self.emit)(Fact::Binding(Binding {
                        scope: c.scope,
                        name: "self".into(),
                        target: None,
                        module: None,
                        imported_name: None,
                        type_name: Some(owner),
                        kind: "parameter",
                        span: span(object),
                    }))?;
                }
            }
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn ruby_barrier(
        &mut self,
        c: &Context,
        reason: &str,
        scope: BarrierScope,
        at: crate::domain::source::SourceSpan,
    ) -> Result<()> {
        let project = matches!(scope, BarrierScope::Project);
        let owner = if matches!(scope, BarrierScope::Owner) {
            c.type_owner.unwrap_or(0)
        } else {
            0
        };
        (self.emit)(Fact::Binding(Binding {
            scope: c.scope,
            name: "@ruby-lookup".into(),
            target: Some(owner),
            module: Some(reason.into()),
            imported_name: None,
            type_name: None,
            kind: if project {
                "ruby_project_barrier"
            } else {
                "ruby_lookup_barrier"
            },
            span: at,
        }))
    }
}
