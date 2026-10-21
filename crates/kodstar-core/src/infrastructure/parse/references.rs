// Purpose: 호출·타입·상속의 사용 위치 사실 추출.
// Pattern: Simple Module.
// Usage: 선언 처리 후 추출 순회가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Fact, SyntaxUse};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::source::SourceSpan;
use crate::infrastructure::parse::{Context, Extractor, imports, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn callee<'a>(&self, node: Node<'a>) -> Option<Node<'a>> {
        node.child_by_field_name(self.config.call_field)
            .or_else(|| node.child_by_field_name("constructor"))
            .or_else(|| node.child_by_field_name("entity"))
            .or_else(|| node.child_by_field_name("type"))
            .or_else(|| node.named_child(0))
    }
    pub(super) fn references(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        let node_kind = node.kind();
        let module_call = matches!(self.language, Language::JavaScript | Language::TypeScript)
            && crate::infrastructure::parse::javascript::imports::dynamic_import(node);
        if self.config.imports.contains(&node_kind) || module_call {
            imports::extract(
                self.language,
                node,
                self.source,
                self.original_source,
                context,
                self.emit,
                self.fact_limit,
            )?;
        }
        let declaration_head = self.language == Language::Elixir
            && (crate::infrastructure::parse::elixir::declaration_head(node, self.source)
                || crate::infrastructure::parse::elixir::capture_target(node, self.source)
                || node.child_by_field_name("target").is_some_and(|n| {
                    matches!(
                        text(n, self.source),
                        "alias" | "import" | "use" | "require" | "defstruct" | "defexception"
                    )
                }));
        let ruby_form = self.ruby_facts(node, context)?;
        if self.config.calls.contains(&node_kind)
            && !matches!(self.language, Language::CommonLisp)
            && !declaration_head
            && !module_call
            && !ruby_form
            && !(self.language == Language::Elixir && self.special_definition(node)?.is_some())
        {
            let callee = self.callee(node);
            if let Some(callee) = callee {
                let mut expression = self.call_text(node, callee)?;
                if self.language == Language::Rust && node_kind == "macro_invocation" {
                    expression.push('!');
                }
                if self.language == Language::Elixir {
                    let mut arity = crate::infrastructure::parse::child_kind(node, &["arguments"])
                        .map_or(0, |n| n.named_child_count());
                    if node.parent().is_some_and(|p| {
                        p.kind() == "binary_operator"
                            && p.child_by_field_name("operator")
                                .is_some_and(|n| text(n, self.source) == "|>")
                            && p.child_by_field_name("right")
                                .is_some_and(|n| n.id() == node.id())
                    }) {
                        arity += 1;
                    }
                    expression.push_str(&format!("/{arity}"));
                }
                if self.language == Language::Erlang {
                    let arity = node
                        .child_by_field_name("args")
                        .map_or(0, |args| args.named_child_count());
                    expression.push_str(&format!("/{arity}"));
                    if let Some(remote) = node.parent().filter(|p| p.kind() == "remote")
                        && let Some(module) = remote
                            .child_by_field_name("module")
                            .and_then(|n| n.child_by_field_name("module"))
                        && module.kind() == "atom"
                    {
                        expression = format!(
                            "@module:{}#{}",
                            text(module, self.source).trim_matches('\''),
                            expression
                        );
                    }
                }
                if !expression.is_empty() {
                    let relation = if self.language == Language::Erlang
                        && crate::infrastructure::parse::erlang::types::type_context(node)
                    {
                        RelationKind::UsesType
                    } else {
                        RelationKind::Calls
                    };
                    self.usage(context, expression, relation, span(callee))?;
                }
            }
        }
        self.ejs_include(node, context)?;
        self.jsx_reference(node, context)?;
        self.type_use(node, context)?;
        self.erlang_facts(node, context)?;
        self.elixir_capture(node, context)?;
        self.php_facts(node, context)?;
        self.pascal_facts(node, context)?;
        self.python_property_read(node, context)?;
        self.python_exports(node, context)?;
        self.callable_values(node, context)?;
        self.variable(node, context)?;
        if self.language == Language::CommonLisp && node_kind == "list_lit" {
            self.lisp_call(node, context)?;
        }
        Ok(())
    }

    pub(super) fn usage(
        &mut self,
        context: &Context,
        expression: String,
        relation: RelationKind,
        location: SourceSpan,
    ) -> Result<()> {
        let expression = if self.language == Language::CommonLisp && expression.starts_with('@') {
            let mut escaped = String::new();
            self.append_signature(&mut escaped, &["@lisp-symbol:", &expression])?;
            escaped
        } else {
            expression
        };
        (self.emit)(Fact::Use(SyntaxUse {
            source: context.source,
            scope: context.scope,
            expression,
            relation,
            span: location,
            condition: context.condition.as_deref().map(str::to_owned),
            type_only: false,
        }))
    }
}
