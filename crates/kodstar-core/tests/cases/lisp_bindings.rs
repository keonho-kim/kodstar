// Purpose: Lisp 함수 공간·lexical 함수·specializer 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn lisp_function_namespace_is_independent_from_variables_and_classes() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp","(defclass target () ()) (defvar target 1) (defun target () nil) (defun caller (target) (target))")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.lisp", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", rows.rows);
    assert!(
        calls[0]
            .target
            .as_ref()
            .is_some_and(|t| t.kind == "function")
    );
    Ok(())
}

#[test]
fn lisp_flet_bodies_and_function_definitions_use_different_environments() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.lisp",
        "(defun target () nil) (defun caller () (flet ((target () (target))) (target)) (target))",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.lisp", "caller"),
    })?;
    for name in ["target", "caller::target"] {
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.qualified_name == name)),
            "{:?}",
            rows.rows
        );
    }
    let local = p.query(Query::Deps {
        selector: selector("main.lisp", "caller::target"),
    })?;
    assert!(relationships(&local).iter().any(|e| {
        e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "target")
    }));
    Ok(())
}

#[test]
fn lisp_labels_are_mutually_recursive_and_macrolet_bindings_are_local() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp","(defun caller () (labels ((first () (second)) (second () (first))) (first)) (macrolet ((local () nil)) (local)))")?;
    p.build(false)?;
    let first = p.query(Query::Deps {
        selector: selector("main.lisp", "caller::first"),
    })?;
    assert!(relationships(&first).iter().any(|e| {
        e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "caller::second")
    }));
    let second = p.query(Query::Deps {
        selector: selector("main.lisp", "caller::second"),
    })?;
    assert!(relationships(&second).iter().any(|e| {
        e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "caller::first")
    }));
    let caller = p.query(Query::Deps {
        selector: selector("main.lisp", "caller"),
    })?;
    assert!(
        relationships(&caller)
            .iter()
            .any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.kind == "macro"))
    );
    Ok(())
}

#[test]
fn lisp_specializers_use_the_class_namespace_and_methods_remain_candidates() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp","(defclass target () ()) (defun target () nil) (defmethod process ((value target)) nil) (defun caller (value) (process value))")?;
    p.build(false)?;
    let method = p.query(Query::Deps {
        selector: selector("main.lisp", "process"),
    })?;
    assert!(
        relationships(&method)
            .iter()
            .any(|e| e.relation == "uses_type"
                && e.target.as_ref().is_some_and(|t| t.kind == "class")),
        "{:?}",
        method.rows
    );
    let caller = p.query(Query::Explain {
        selector: selector("main.lisp", "caller"),
    })?;
    assert!(
        relationships(&caller).iter().any(|e| e.relation == "calls"
            && e.resolution == "candidate"
            && e.reason
                .as_ref()
                .is_some_and(|r| r.contains("method selection"))),
        "{:?}",
        caller.rows
    );
    Ok(())
}

#[test]
fn lisp_standard_macro_definers_preserve_compiler_and_function_namespaces() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp","(defun target (value) value) (defun optimizer (value) value) (define-compiler-macro target (&whole form value) (optimizer value)) (define-setf-expander target (value) (optimizer value)) (define-modify-macro bump (&optional (delta 1)) target) (defun caller () (target 1) (bump value))")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.lisp", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "resolved"
            && e.target
                .as_ref()
                .is_some_and(|t| t.name == "target" && t.kind == "function")),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.name == "bump" && t.kind == "macro")),
        "{:?}",
        rows.rows
    );
    let symbols = p.query(Query::Symbols {
        file: "main.lisp".into(),
    })?;
    assert_eq!(
        symbols
            .rows
            .iter()
            .filter(|r| matches!(r,QueryRow::Symbol(s) if s.name=="target" && s.kind=="macro"))
            .count(),
        2,
        "{:?}",
        symbols.rows
    );
    Ok(())
}

#[test]
fn lisp_expander_lambda_lists_do_not_become_calls_and_symbol_macros_are_values() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp","(defun optimizer (x) x) (define-compiler-macro calc (&optional (value 1)) (optimizer value)) (defsetf place (object) (stored) (optimizer stored)) (define-symbol-macro current (optimizer 1))")?;
    p.build(false)?;
    for name in ["calc", "place"] {
        let rows = p.query(Query::Deps {
            selector: selector("main.lisp", name),
        })?;
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.name == "optimizer")),
            "{:?}",
            rows.rows
        );
        assert!(
            !relationships(&rows).iter().any(|e| e.relation == "calls"
                && ["&optional", "value", "object", "stored"].contains(&e.expression.as_str())),
            "{:?}",
            rows.rows
        );
    }
    let symbols = p.query(Query::Symbols {
        file: "main.lisp".into(),
    })?;
    assert!(
        symbols
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="current" && s.kind=="field")),
        "{:?}",
        symbols.rows
    );
    Ok(())
}
