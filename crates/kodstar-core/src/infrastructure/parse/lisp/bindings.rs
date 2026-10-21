// Purpose: Lisp 지역 함수의 lexical scope와 lambda-list binding·서명 추출.
// Pattern: Simple Module.
// Usage: 선언·매개변수 순회가 실제 함수 form을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/lisp/mod.rs, crates/kodstar-core/src/infrastructure/parse/definitions.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact, Scope};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::resolve::lisp::key;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::lisp::form_value;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

#[derive(Clone, Copy)]
enum LocalMode {
    Flet,
    Labels,
    Macrolet,
}

fn mode(node: Node<'_>, source: &[u8]) -> Option<LocalMode> {
    if node.kind() != "list_lit" {
        return None;
    }
    let head = form_value(node, 0).filter(|n| n.kind() == "sym_lit")?;
    match key(text(head, source)).as_str() {
        "FLET" => Some(LocalMode::Flet),
        "LABELS" => Some(LocalMode::Labels),
        "MACROLET" => Some(LocalMode::Macrolet),
        _ => None,
    }
}

fn local_mode(node: Node<'_>, source: &[u8]) -> Option<LocalMode> {
    let bindings = node.parent().filter(|n| n.kind() == "list_lit")?;
    let form = bindings.parent().filter(|n| n.kind() == "list_lit")?;
    if form_value(form, 1)?.id() != bindings.id() {
        return None;
    }
    mode(form, source)
}

pub(in crate::infrastructure::parse) fn local_parameters<'t>(
    node: Node<'t>,
    source: &[u8],
) -> Option<Node<'t>> {
    local_mode(node, source)?;
    form_value(node, 1).filter(|n| n.kind() == "list_lit")
}

fn parameters<'t>(node: Node<'t>, source: &[u8]) -> Option<Node<'t>> {
    if node.kind() == "defun" {
        let header = crate::infrastructure::parse::child_kind(node, &["defun_header"])?;
        header.child_by_field_name("lambda_list")
    } else {
        local_parameters(node, source)
            .or_else(|| crate::infrastructure::parse::lisp::definers::parameters(node, source))
    }
}

fn parameter_name(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "sym_lit" {
        return Some(node);
    }
    let first = form_value(node, 0)?;
    if first.kind() == "sym_lit" {
        Some(first)
    } else {
        form_value(first, 1).filter(|n| n.kind() == "sym_lit")
    }
}

fn method(node: Node<'_>, source: &[u8]) -> bool {
    crate::infrastructure::parse::child_kind(node, &["defun_header"])
        .and_then(|n| n.child_by_field_name("keyword"))
        .is_some_and(|n| key(text(n, source)) == "DEFMETHOD")
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn lisp_scope(
        &mut self,
        node: Node<'_>,
        context: &mut Context,
    ) -> Result<()> {
        if self.language != Language::CommonLisp
            || mode(node, self.source).is_none()
            || local_mode(node, self.source).is_some()
        {
            return Ok(());
        }
        let scope = self.allocate()?;
        (self.emit)(Fact::Scope(Scope {
            local_id: scope,
            parent: Some(context.scope),
            span: span(node),
        }))?;
        context.lisp_outer_scope = context.scope;
        context.scope = scope;
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn lisp_local_definition<'t>(
        &self,
        node: Node<'t>,
    ) -> Option<(Node<'t>, SymbolKind)> {
        if self.language != Language::CommonLisp || node.kind() != "list_lit" {
            return None;
        }
        let mode = local_mode(node, self.source)?;
        let name = form_value(node, 0).filter(|n| n.kind() == "sym_lit")?;
        local_parameters(node, self.source)?;
        Some((
            name,
            if matches!(mode, LocalMode::Macrolet) {
                SymbolKind::Macro
            } else {
                SymbolKind::Function
            },
        ))
    }

    pub(in crate::infrastructure::parse) fn lisp_nonrecursive_local(&self, node: Node<'_>) -> bool {
        self.language == Language::CommonLisp
            && matches!(
                local_mode(node, self.source),
                Some(LocalMode::Flet | LocalMode::Macrolet)
            )
    }

    pub(in crate::infrastructure::parse) fn lisp_parameters(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        let Some(parameters) = parameters(node, self.source) else {
            return Ok(());
        };
        let mut cursor = parameters.walk();
        let mut required = true;
        for parameter in parameters
            .named_children(&mut cursor)
            .filter(|n| !matches!(n.kind(), "comment" | "block_comment"))
        {
            let Some(name) = parameter_name(parameter) else {
                continue;
            };
            let raw = text(name, self.source);
            if raw.starts_with('&') {
                required = false;
                continue;
            }
            let ty = if required && method(node, self.source) && parameter.kind() == "list_lit" {
                form_value(parameter, 1).filter(|n| matches!(n.kind(), "sym_lit" | "package_lit"))
            } else {
                None
            };
            let name = self.bounded_text(raw)?;
            let type_name = ty
                .map(|n| self.bounded_text(text(n, self.source)))
                .transpose()?;
            (self.emit)(Fact::Binding(Binding {
                scope: context.scope,
                name,
                target: None,
                module: None,
                imported_name: None,
                type_name,
                kind: "parameter",
                span: span(parameter),
            }))?;
            if let Some(ty) = ty {
                self.usage(
                    context,
                    self.bounded_text(text(ty, self.source))?,
                    RelationKind::UsesType,
                    span(ty),
                )?;
            }
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn lisp_signature(
        &self,
        node: Node<'_>,
    ) -> Result<String> {
        let prefix =
            match crate::infrastructure::parse::lisp::definers::binding_kind(node, self.source) {
                Some("lisp_compiler_macro") => "compiler_macro",
                Some("lisp_setf_expander") => "setf_expander",
                _ => "",
            };
        let mut result = String::new();
        self.append_signature(&mut result, &[prefix, "("])?;
        let Some(parameters) = parameters(node, self.source) else {
            self.append_signature(&mut result, &[")"])?;
            return Ok(result);
        };
        let mut cursor = parameters.walk();
        let mut required = true;
        for parameter in parameters
            .named_children(&mut cursor)
            .filter(|n| !matches!(n.kind(), "comment" | "block_comment"))
        {
            let Some(name) = parameter_name(parameter) else {
                continue;
            };
            let raw = text(name, self.source);
            if raw.starts_with('&') {
                required = false;
            }
            self.append_signature(&mut result, &[raw])?;
            if required
                && method(node, self.source)
                && parameter.kind() == "list_lit"
                && let Some(ty) = form_value(parameter, 1)
            {
                self.append_signature(
                    &mut result,
                    &[
                        ":",
                        if matches!(ty.kind(), "sym_lit" | "package_lit") {
                            text(ty, self.source)
                        } else {
                            "<specializer>"
                        },
                    ],
                )?;
            }
            self.append_signature(&mut result, &[","])?;
        }
        self.append_signature(&mut result, &[")"])?;
        Ok(result)
    }
}
