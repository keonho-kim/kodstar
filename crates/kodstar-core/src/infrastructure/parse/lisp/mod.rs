// Purpose: Lisp reader 인용·선언 데이터와 호출·함수 참조 영역 구분.
// Pattern: Simple Module.
// Usage: 선언 진입과 Lisp list 참조 순회가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/infrastructure/parse/references.rs.

pub(super) mod bindings;
pub(super) mod definers;
pub(super) mod definitions;
pub(super) mod packages;

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::relations::RelationKind;
use crate::domain::resolve::lisp::key;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

#[derive(Clone, Copy, Default)]
pub(in crate::infrastructure::parse) enum Region {
    #[default]
    Code,
    Literal,
    Backquote(u16),
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn lisp_region(
        &mut self,
        node: Node<'_>,
        context: &mut Context,
    ) -> Result<bool> {
        if matches!(context.lisp_region, Region::Literal) {
            return Ok(false);
        }
        context.lisp_region = match (context.lisp_region, node.kind()) {
            (Region::Code, "quoting_lit") => Region::Literal,
            (Region::Code, "syn_quoting_lit") => Region::Backquote(1),
            (Region::Backquote(depth), "syn_quoting_lit") => Region::Backquote(
                depth
                    .checked_add(1)
                    .ok_or_else(|| Error::new(ErrorKind::Limit, "Lisp backquote nesting limit"))?,
            ),
            (Region::Backquote(1), "unquoting_lit" | "unquote_splicing_lit") => Region::Code,
            (Region::Backquote(depth), "unquoting_lit" | "unquote_splicing_lit") => {
                Region::Backquote(depth - 1)
            }
            (Region::Code, "unquoting_lit" | "unquote_splicing_lit") => {
                return Err(Error::operation("Lisp comma outside backquote"));
            }
            (region, _) => region,
        };
        if !matches!(context.lisp_region, Region::Code) {
            return Ok(false);
        }
        if node.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                "include_reader_macro" | "read_cond_lit" | "splicing_read_cond_lit"
            ) && p
                .child_by_field_name("condition")
                .is_some_and(|n| n.id() == node.id())
        }) {
            context.lisp_region = Region::Literal;
            return Ok(false);
        }
        if crate::infrastructure::parse::lisp::definers::declaration_data(node, self.source) {
            context.lisp_region = Region::Literal;
            return Ok(false);
        }
        let first = form_value(node, 0);
        let head = if node.kind() == "list_lit" {
            if let Some(first) = first.filter(|n| n.kind() == "sym_lit") {
                let raw = text(first, self.source);
                if raw.len() > self.fact_limit {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        "Lisp form head exceeds fact_batch_bytes",
                    ));
                }
                key(raw)
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        if node.kind() == "list_lit" && head == "QUOTE" {
            context.lisp_region = Region::Literal;
            return Ok(false);
        }
        if node.kind() == "var_quoting_lit" || node.kind() == "list_lit" && head == "FUNCTION" {
            let value = if node.kind() == "var_quoting_lit" {
                first
            } else {
                form_value(node, 1)
            };
            if let Some(value) = value.filter(|n| matches!(n.kind(), "sym_lit" | "package_lit")) {
                self.usage(
                    context,
                    self.bounded_text(text(value, self.source))?,
                    RelationKind::References,
                    span(value),
                )?;
                context.lisp_region = Region::Literal;
                return Ok(false);
            }
        }
        if node.kind() == "list_lit"
            && node.parent().is_some_and(|p| {
                if crate::infrastructure::parse::lisp::bindings::local_parameters(p, self.source)
                    .is_some_and(|n| n.id() == node.id())
                {
                    return true;
                }
                p.kind() == "defun_header"
                    && p.child_by_field_name("lambda_list")
                        .is_some_and(|n| n.id() == node.id())
                    || p.kind() == "list_lit"
                        && form_value(p, 0)
                            .filter(|n| n.kind() == "sym_lit")
                            .is_some_and(|n| {
                                matches!(
                                    key(text(n, self.source)).as_str(),
                                    "DEFCLASS" | "DEFSTRUCT" | "DEFINE-CONDITION"
                                )
                            })
            })
        {
            context.lisp_region = Region::Literal;
            return Ok(false);
        }
        Ok(true)
    }

    pub(in crate::infrastructure::parse) fn lisp_call(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        let Some(first) =
            form_value(node, 0).filter(|n| matches!(n.kind(), "sym_lit" | "package_lit"))
        else {
            return Ok(());
        };
        if crate::infrastructure::parse::lisp::definers::global_name(node, self.source).is_some()
            || binding_pair(node, self.source)
        {
            return Ok(());
        }
        let name = self.bounded_text(text(first, self.source))?;
        if matches!(
            key(&name).as_str(),
            "DEFUN"
                | "DEFCLASS"
                | "DEFMETHOD"
                | "DEFMACRO"
                | "DEFGENERIC"
                | "DEFSTRUCT"
                | "DEFVAR"
                | "DEFPARAMETER"
                | "DEFCONSTANT"
                | "DEFTYPE"
                | "DEFINE-CONDITION"
                | "IN-PACKAGE"
                | "DEFPACKAGE"
                | "LET"
                | "LET*"
                | "FLET"
                | "LABELS"
                | "MACROLET"
                | "IF"
                | "PROGN"
                | "QUOTE"
                | "LAMBDA"
                | "SETQ"
                | "SETF"
                | "DECLARE"
                | "FUNCTION"
                | "COND"
                | "CASE"
                | "BLOCK"
                | "RETURN-FROM"
                | "TAGBODY"
                | "GO"
                | "UNWIND-PROTECT"
                | "CATCH"
                | "THROW"
        ) {
            return Ok(());
        }
        self.usage(context, name, RelationKind::Calls, span(first))
    }
}

fn binding_pair(node: Node<'_>, source: &[u8]) -> bool {
    let Some(parent) = node.parent().filter(|n| n.kind() == "list_lit") else {
        return false;
    };
    if form_value(parent, 0)
        .filter(|n| n.kind() == "sym_lit")
        .is_some_and(|n| matches!(key(text(n, source)).as_str(), "COND" | "CASE" | "TYPECASE"))
    {
        return true;
    }
    let Some(form) = parent.parent().filter(|n| n.kind() == "list_lit") else {
        return false;
    };
    form_value(form, 1).is_some_and(|n| n.id() == parent.id())
        && form_value(form, 0)
            .filter(|n| n.kind() == "sym_lit")
            .is_some_and(|n| {
                matches!(
                    key(text(n, source)).as_str(),
                    "LET" | "LET*" | "FLET" | "LABELS" | "MACROLET" | "DO" | "DO*"
                )
            })
}

pub(in crate::infrastructure::parse) fn form_value(
    node: Node<'_>,
    index: usize,
) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|n| !n.is_extra() && !matches!(n.kind(), "comment" | "block_comment"))
        .nth(index)
}
