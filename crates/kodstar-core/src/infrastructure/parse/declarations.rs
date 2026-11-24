// Purpose: 정의·scope·서명·조건의 구문 사실 추출.
// Pattern: Simple Module.
// Usage: 추출 순회가 각 노드 진입 시 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/infrastructure/parse/syntax.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::{Fact, Scope};
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use std::sync::Arc;
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn allocate(&mut self) -> Result<u32> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::Limit, "too many file-local syntax objects"))?;
        Ok(self.next)
    }

    pub(super) fn enter(
        &mut self,
        node: Node<'_>,
        mut context: Context,
        parent: Option<Node<'_>>,
        top_level: bool,
    ) -> Result<Context> {
        let node_kind = node.kind();
        if !node.is_named() {
            return Ok(context);
        }
        if parent.is_some()
            && let Some(projection) = self.projection
            && let Some(condition) = projection.at(node.start_byte(), self.fact_limit)?
            && context.projection_guard.as_deref() != Some(condition.as_str())
        {
            context.projection_guard = Some(Arc::from(condition.as_str()));
            context.condition = Some(Arc::from(match context.condition.as_deref() {
                Some(parent) if parent != condition => {
                    self.bounded_text(&format!("({parent}) && ({condition})"))?
                }
                _ => condition,
            }));
        }
        if !self.language_context(node, &mut context)? {
            return Ok(context);
        }
        self.htmx_context(node, &mut context)?;
        self.namespace_context(node, &mut context, top_level)?;
        self.lisp_package_context(node, &mut context)?;
        self.enum_member(node, &context)?;
        self.field(node, &context)?;
        self.go_embedding(node, &context)?;
        if matches!(
            node_kind,
            "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_else"
        ) {
            context.condition = self.native_condition(node)?.map(Arc::from);
        }
        if matches!(
            node_kind,
            "package_declaration" | "package_header" | "package_clause" | "module_attribute"
        ) {
            let name = node.child_by_field_name("name").or_else(|| {
                child_kind(
                    node,
                    &[
                        "scoped_identifier",
                        "qualified_identifier",
                        "identifier",
                        "package_identifier",
                        "atom",
                    ],
                )
            });
            if let Some(name) = name {
                let name = self.bounded_text(text(name, self.source).trim_matches('\''))?;
                let name = if self.language == Language::Kotlin {
                    self.language.lookup_key(&name)
                } else {
                    name
                };
                (self.emit)(Fact::Module {
                    name,
                    declaration_only: self.language == Language::Go
                        && node_kind == "package_clause",
                })?;
            }
        }
        self.implementation(node, &mut context)?;
        self.cxx_owner(node, &mut context)?;
        let special = self.special_definition(node)?;
        let function = self.config.functions.contains(&node_kind)
            || special.as_ref().is_some_and(|(_, kind)| {
                matches!(
                    kind,
                    SymbolKind::Function
                        | SymbolKind::Method
                        | SymbolKind::Macro
                        | SymbolKind::Constructor
                )
            });
        let class = self.config.classes.contains(&node_kind)
            || special.as_ref().is_some_and(|(_, kind)| {
                !matches!(
                    kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Macro
                )
            });
        let is_arrow = matches!(
            node_kind,
            "arrow_function" | "function_expression" | "anonymous_function"
        ) && matches!(
            self.language,
            Language::JavaScript | Language::TypeScript | Language::Php
        );
        let extension = self.language == Language::Swift
            && crate::infrastructure::parse::ownership::extension(node);
        if (function || class || is_arrow) && !extension {
            context = self.define(
                node,
                context,
                crate::infrastructure::parse::definitions::Declaration {
                    function,
                    arrow: is_arrow,
                    special,
                },
            )?;
        } else if matches!(
            node_kind,
            "statement_block" | "block" | "compound_statement"
        ) && !matches!(
            self.language,
            Language::Python | Language::Ruby | Language::Elixir | Language::CommonLisp
        ) {
            let id = self.allocate()?;
            (self.emit)(Fact::Scope(Scope {
                local_id: id,
                parent: Some(context.scope),
                span: span(node),
            }))?;
            context.scope = id;
        }
        self.references(node, &context, parent)?;
        Ok(context)
    }
}
