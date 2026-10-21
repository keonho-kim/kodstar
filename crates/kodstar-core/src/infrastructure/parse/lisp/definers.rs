// Purpose: 표준 Lisp definer의 이름·종류·lambda-list와 선언 데이터 범위 분류.
// Pattern: Simple Module.
// Usage: 전역 정의·binding·reader 영역 처리가 같은 명시적 분류를 사용.
// Related: crates/kodstar-core/src/infrastructure/parse/lisp/definitions.rs, crates/kodstar-core/src/infrastructure/parse/lisp/bindings.rs.

use crate::domain::resolve::lisp::key;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::lisp::form_value;
use crate::infrastructure::parse::{child_kind, text};
use tree_sitter::Node;

fn head(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() != "list_lit" {
        return None;
    }
    form_value(node, 0)
        .filter(|n| n.kind() == "sym_lit")
        .map(|n| key(text(n, source)))
}

pub(in crate::infrastructure::parse) fn global_name<'a>(
    node: Node<'a>,
    source: &[u8],
) -> Option<(Node<'a>, SymbolKind)> {
    if node.kind() == "defun" {
        let header = child_kind(node, &["defun_header"])?;
        let name = header.child_by_field_name("function_name")?;
        let kind = match header
            .child_by_field_name("keyword")
            .map(|n| key(text(n, source)))
            .as_deref()
        {
            Some("DEFMACRO") => SymbolKind::Macro,
            Some("DEFMETHOD") => SymbolKind::Method,
            _ => SymbolKind::Function,
        };
        return Some((name, kind));
    }
    let head = head(node, source)?;
    let kind = match head.as_str() {
        "DEFCLASS" | "DEFINE-CONDITION" => SymbolKind::Class,
        "DEFSTRUCT" => SymbolKind::Struct,
        "DEFTYPE" => SymbolKind::Type,
        "DEFPACKAGE" => SymbolKind::Module,
        "DEFVAR" | "DEFPARAMETER" | "DEFCONSTANT" | "DEFINE-SYMBOL-MACRO" => SymbolKind::Field,
        "DEFINE-MODIFY-MACRO" | "DEFINE-COMPILER-MACRO" | "DEFINE-SETF-EXPANDER" | "DEFSETF" => {
            SymbolKind::Macro
        }
        _ => return None,
    };
    let mut name = form_value(node, 1)?;
    if name.kind() == "list_lit" && kind != SymbolKind::Macro {
        name = form_value(name, 0)?;
    }
    Some((name, kind))
}

pub(in crate::infrastructure::parse) fn binding_kind(
    node: Node<'_>,
    source: &[u8],
) -> Option<&'static str> {
    match head(node, source)?.as_str() {
        "DEFINE-COMPILER-MACRO" => Some("lisp_compiler_macro"),
        "DEFINE-SETF-EXPANDER" | "DEFSETF" => Some("lisp_setf_expander"),
        _ => None,
    }
}

pub(in crate::infrastructure::parse) fn parameters<'a>(
    node: Node<'a>,
    source: &[u8],
) -> Option<Node<'a>> {
    if !matches!(
        head(node, source)?.as_str(),
        "DEFINE-MODIFY-MACRO"
            | "DEFINE-COMPILER-MACRO"
            | "DEFINE-SETF-EXPANDER"
            | "DEFSETF"
            | "DEFTYPE"
    ) {
        return None;
    }
    form_value(node, 2).filter(|n| n.kind() == "list_lit")
}

pub(in crate::infrastructure::parse) fn declaration_data(node: Node<'_>, source: &[u8]) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() == "defun_header"
        && parent
            .child_by_field_name("function_name")
            .is_some_and(|n| n.id() == node.id())
    {
        return true;
    }
    let Some(head) = head(parent, source) else {
        return false;
    };
    if parameters(parent, source).is_some_and(|n| n.id() == node.id()) {
        return true;
    }
    if matches!(
        head.as_str(),
        "DEFINE-COMPILER-MACRO" | "DEFINE-SETF-EXPANDER" | "DEFSETF" | "DEFINE-MODIFY-MACRO"
    ) && form_value(parent, 1).is_some_and(|n| n.id() == node.id())
    {
        return true;
    }
    head == "DEFSETF" && form_value(parent, 3).is_some_and(|n| n.id() == node.id())
}
