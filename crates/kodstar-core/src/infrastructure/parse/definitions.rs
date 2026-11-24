// Purpose: Single definition, scope, binding and declaration metadata construction.
// Pattern: Simple Module.
// Usage: The declaration walker passes one classified declaration and its context.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/infrastructure/parse/declaration_metadata.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::{Binding, Definition, Fact, Scope, SyntaxUse};
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::syntax::{kind_for, name_node};
use crate::infrastructure::parse::{Context, Extractor, span, text};
use std::sync::Arc;
use tree_sitter::Node;

pub(super) struct Declaration {
    pub function: bool,
    pub arrow: bool,
    pub special: Option<(String, SymbolKind)>,
}

fn kind_for_namespace(language: Language, kind: SymbolKind) -> bool {
    kind == SymbolKind::Module
        && matches!(
            language,
            Language::CSharp | Language::Php | Language::Cpp | Language::Elixir
        )
}

impl Extractor<'_> {
    pub(super) fn define(
        &mut self,
        node: Node<'_>,
        mut context: Context,
        declaration: Declaration,
    ) -> Result<Context> {
        let Declaration {
            function,
            arrow: is_arrow,
            special,
        } = declaration;
        let declaration_source = context.source;
        let node_kind = node.kind();
        let mut name = special.as_ref().map(|(name, _)| name.clone());
        if name.is_none() {
            name = name_node(node)
                .map(|name| self.bounded_text(text(name, self.source)))
                .transpose()?;
        }
        let class_expression = self.class_expression(node);
        if (is_arrow || class_expression) && name.is_none() {
            name = node
                .parent()
                .and_then(|parent| {
                    parent
                        .child_by_field_name("name")
                        .or_else(|| parent.child_by_field_name("left"))
                })
                .map(|name| self.bounded_text(text(name, self.source)))
                .transpose()?;
        }
        let name = name.unwrap_or_else(|| format!("<anonymous@{}>", node.start_byte()));
        let placement = self.lisp_placement(node)?;
        let id = self.allocate()?;
        let parent_scope = placement.as_ref().map_or(context.scope, |p| p.scope);
        let lexical_parent = if self.lisp_nonrecursive_local(node) {
            context.lisp_outer_scope
        } else if placement.is_some() {
            context.scope
        } else if self.language == Language::Ruby && function {
            context.type_owner.unwrap_or(0)
        } else if self.language == Language::Python && context.member_scope {
            context.member_outer
        } else {
            parent_scope
        };
        (self.emit)(Fact::Scope(Scope {
            local_id: id,
            parent: Some(lexical_parent),
            span: span(node),
        }))?;
        let kind = special
            .map(|(_, kind)| kind)
            .unwrap_or_else(|| kind_for(node, function || is_arrow, context.member_scope));
        let qualified_name = if kind_for_namespace(self.language, kind) {
            name.replace(['.', '\\'], "::")
        } else {
            name.clone()
        };
        let base = placement.as_ref().map_or(&context.prefix, |p| &p.prefix);
        let prefix = base.child(&qualified_name, self.fact_limit)?;
        let qualified = prefix.render()?;
        let signature = if function || is_arrow {
            self.signature(node)?
        } else {
            String::new()
        };
        let own_condition = self.condition(node)?;
        let condition = match (own_condition, context.condition.as_deref()) {
            (Some(own), Some(parent)) if self.projection.is_some() && own != parent => {
                Some(self.bounded_text(&format!("({parent}) && ({own})"))?)
            }
            (Some(own), _) => Some(own),
            (None, parent) => parent.map(str::to_owned),
        };
        let payload = std::mem::size_of::<Fact>()
            .saturating_add(name.len())
            .saturating_add(qualified.capacity())
            .saturating_add(signature.capacity())
            .saturating_add(condition.as_ref().map_or(0, String::len));
        if payload > self.fact_limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "definition exceeds fact_batch_bytes",
            ));
        }
        let exported = self.definition_exported(node, &name);
        (self.emit)(Fact::Definition(Definition {
            local_id: id,
            owner: Some(placement.as_ref().map_or(context.owner, |p| p.owner)),
            scope: parent_scope,
            name: name.clone(),
            qualified_name: qualified,
            kind,
            signature,
            condition: condition.clone(),
            span: span(node),
            exported,
        }))?;
        if class_expression {
            self.class_assignment(node, id, parent_scope)?;
        }
        if !(function
            && context.member_scope
            && matches!(
                self.language,
                Language::Rust | Language::Go | Language::JavaScript | Language::TypeScript
            ))
        {
            let external_module = self.language == Language::Rust
                && node_kind == "mod_item"
                && node.child_by_field_name("body").is_none();
            let module = if external_module {
                Some(format!("self::{name}"))
            } else {
                None
            };
            let binding_name = if kind == SymbolKind::Macro
                && matches!(self.language, Language::Rust | Language::Erlang)
            {
                format!("{name}!")
            } else if self.language == Language::Elixir && function {
                self.elixir_binding_name(node, &name)
            } else {
                name
            };
            let local_definition = self.lisp_local_definition(node).is_some();
            (self.emit)(Fact::Binding(Binding {
                scope: if class_expression { id } else { parent_scope },
                name: binding_name,
                target: (!external_module).then_some(id),
                module: module.clone(),
                imported_name: None,
                type_name: None,
                kind: if external_module {
                    "import"
                } else if self.language == Language::CommonLisp && local_definition {
                    "lisp_local_function"
                } else if self.language == Language::Php && function && context.member_scope {
                    "php_method"
                } else if self.language == Language::CommonLisp
                    && let Some(binding_kind) =
                        crate::infrastructure::parse::lisp::definers::binding_kind(
                            node,
                            self.source,
                        )
                {
                    binding_kind
                } else if self.language == Language::CommonLisp && kind == SymbolKind::Module {
                    "lisp_package_definition"
                } else if self.language == Language::CommonLisp && placement.is_some() {
                    if kind == SymbolKind::Field {
                        "lisp_global_value"
                    } else if function {
                        "lisp_global_function"
                    } else {
                        "lisp_global_type"
                    }
                } else if self.language == Language::Scala
                    && matches!(node.kind(), "object_definition" | "given_definition")
                {
                    if node.kind() == "object_definition" {
                        "scala_object"
                    } else {
                        "scala_given"
                    }
                } else if matches!(self.language, Language::Rust | Language::Erlang)
                    && kind == SymbolKind::Macro
                {
                    "macro"
                } else if matches!(
                    self.language,
                    Language::Rust | Language::CommonLisp | Language::Php | Language::Scala
                ) && kind == SymbolKind::Field
                {
                    "value"
                } else if function || is_arrow {
                    "function"
                } else {
                    "type"
                },
                span: span(node),
            }))?;
            if let Some(module) = module {
                (self.emit)(Fact::Use(SyntaxUse {
                    source: context.source,
                    scope: parent_scope,
                    expression: format!("@module:{module}"),
                    relation: crate::domain::relations::RelationKind::Imports,
                    span: span(node),
                    condition: condition.clone(),
                    type_only: false,
                }))?;
            }
        }
        if function || is_arrow {
            self.hono_handler(node, id, parent_scope, declaration_source)?;
        }
        if function && context.member_scope {
            self.owner_hint(id, node, &context)?;
        }
        context.implementation_owner = None;
        self.lisp_package_definition(node, id)?;
        context.declaration_name = name_node(node).map(|name| (name.start_byte(), name.end_byte()));
        context.scope = id;
        self.rust_constructor(node, id, parent_scope)?;
        self.rust_macro_export(node, id)?;
        self.partial_type(node, id, parent_scope)?;
        self.type_parameters(node, &context)?;
        context.source = id;
        context.owner = id;
        context.prefix = prefix;
        context.condition = condition.map(Arc::from);
        context.member_scope =
            !(function || is_arrow || matches!(kind, SymbolKind::Module | SymbolKind::Field))
                || self.language == Language::Ruby && kind == SymbolKind::Module;
        if context.member_scope {
            if self.language == Language::Ruby {
                context.ruby_singleton_depth = 0;
            }
            context.member_outer = lexical_parent;
            context.type_name = Some(context.prefix.clone());
            context.type_owner = Some(id);
        } else if function || is_arrow {
            if kind == SymbolKind::Function && !is_arrow {
                context.type_name = None;
                context.type_owner = None;
            }
            context.function_scope = id;
            self.parameters(node, &context)?;
            self.go_signature(node, &context)?;
            self.python_callable(node, &context)?;
            self.php_callable(node, &context)?;
            self.native_return(node, &context)?;
            self.java_method_parameters(node, &context)?;
            self.kotlin_callable(node, &context)?;
            self.swift_callable(node, &context)?;
            self.scala_callable(node, &context)?;
            self.elixir_callable(node, parent_scope, &context)?;
            self.cxx_callable(node, &context)?;
        }
        self.elixir_container(node, &context)?;
        self.cxx_type(node, &context)?;
        self.inheritance(node, &context)?;
        self.ruby_metadata(node, &context)?;
        self.kotlin_declaration_binding(node, &context, parent_scope, kind)?;
        self.swift_property(node, &context)?;
        self.scala_property(node, &context)?;
        self.decorators(node, &context)?;

        Ok(context)
    }
}
