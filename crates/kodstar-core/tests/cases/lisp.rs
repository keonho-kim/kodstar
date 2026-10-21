// Purpose: Common Lisp reader 이름·이스케이프·인용과 실제 사용 위치 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn lisp_default_reader_case_preserves_escaped_symbols_and_operators() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp",r"(defun mixed () nil) (defun |MiXeD| () nil) (defun upi< () nil) (defun caller () (MIXED) (|MiXeD|) (upi<))")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.lisp", "CALLER"),
    })?;
    for target in ["mixed", "|MiXeD|", "upi<"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "calls"
                    && e.target.as_ref().is_some_and(|t| t.name == target)),
            "{target}: {:?}",
            rows.rows
        );
    }
    let searched = p.query(Query::Search {
        name: "MIXED".into(),
        prefix: false,
    })?;
    assert!(
        searched
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="mixed"))
    );
    assert!(
        !searched
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="|MiXeD|"))
    );
    Ok(())
}

#[test]
fn lisp_quoted_definitions_and_calls_are_data_but_unquotes_are_code() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp",r"(defun target () nil) (defun caller () '(target) (quote (target)) '(defun ghost () (target)) `(target ,(target)) #'target)")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.lisp", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls")
            .map(|e| e.count)
            .sum::<u64>(),
        1,
        "{:?}",
        rows.rows
    );
    assert!(relationships(&rows).iter().any(
        |e| e.relation == "references" && e.target.as_ref().is_some_and(|t| t.name == "target")
    ));
    let defined = p.query(Query::Symbols {
        file: "main.lisp".into(),
    })?;
    assert!(
        !defined
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="ghost"))
    );
    Ok(())
}

#[test]
fn lisp_lambda_lists_and_binding_pairs_are_not_function_calls() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.lisp",
        "(defun target () nil) (defun caller (target) (let ((target 1)) (target)))",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.lisp", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls")
            .map(|e| e.count)
            .sum::<u64>(),
        1,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn lisp_comment_positions_and_nested_backquotes_preserve_only_evaluated_uses() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp","(defun target () nil) (defun caller () (; comment\n target) `(data `(data ,(target)) ,@(target)))")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.lisp", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls")
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn lisp_source_package_tokens_do_not_become_engine_file_markers() -> TestResult {
    let p = Project::new()?;
    p.write("target.lisp", "(defun target () nil)")?;
    p.write("main.lisp", "(defun caller () (@file:target.lisp))")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.lisp", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "@file:target.lisp"
                && e.resolution == "unresolved"
                && e.target.is_none()),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn lisp_reader_feature_expressions_are_data_not_function_calls() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.lisp",
        "(defun and () nil) #+(and debug test) (defun caller () nil)",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.lisp", "main.lisp"),
    })?;
    assert!(
        !relationships(&rows).iter().any(|e| e.relation == "calls"),
        "{:?}",
        rows.rows
    );
    Ok(())
}
