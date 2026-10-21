// Purpose: Lisp reader package 구간·파일 간 이름·명시적 package 경로 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn lisp_same_package_cross_file_and_multiple_reader_sections_are_isolated() -> TestResult {
    let p = Project::new()?;
    p.write("left.lisp", "(in-package :left) (defun target () nil)")?;
    p.write("main.lisp", "(in-package :left) (defun caller () (target)) (in-package :right) (defun isolated () (target) (left::target))")?;
    p.build(false)?;
    let left = p.query(Query::Deps {
        selector: selector("main.lisp", "LEFT::caller"),
    })?;
    assert!(
        relationships(&left).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.file == "left.lisp" && t.module == "LEFT")),
        "{:?}",
        left.rows
    );
    let right = p.query(Query::Explain {
        selector: selector("main.lisp", "RIGHT::isolated"),
    })?;
    assert!(
        relationships(&right)
            .iter()
            .any(|e| e.expression == "target" && e.resolution == "unresolved"),
        "{:?}",
        right.rows
    );
    assert!(
        relationships(&right)
            .iter()
            .any(|e| e.expression == "left::target"
                && e.target.as_ref().is_some_and(|t| t.module == "LEFT")),
        "{:?}",
        right.rows
    );
    Ok(())
}

#[test]
fn lisp_qualified_definition_home_does_not_change_the_body_reader_package() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp", "(in-package :left) (defun local () nil) (defun right::foreign () (local)) (in-package :right) (defun caller () (foreign))")?;
    p.build(false)?;
    let foreign = p.query(Query::Deps {
        selector: selector("main.lisp", "RIGHT::foreign"),
    })?;
    assert!(
        relationships(&foreign).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "LEFT::local")),
        "{:?}",
        foreign.rows
    );
    let caller = p.query(Query::Deps {
        selector: selector("main.lisp", "RIGHT::caller"),
    })?;
    assert!(
        relationships(&caller).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "RIGHT::foreign" && t.module == "RIGHT")),
        "{:?}",
        caller.rows
    );
    Ok(())
}

#[test]
fn lisp_exports_imports_nicknames_and_shadowing_control_access() -> TestResult {
    let p = Project::new()?;
    p.write("packages.lisp","(defpackage :api (:nicknames :a) (:export :public)) (defpackage :client (:use :api) (:import-from :api :private)) (defpackage :blocked (:use :api) (:shadow :public)) (defpackage :bridge (:import-from :api :public) (:export :public))")?;
    p.write(
        "api.lisp",
        "(in-package :api) (defun public () nil) (defun private () nil)",
    )?;
    p.write("client.lisp","(in-package :client) (defun caller () (public) (private) (a:public) (a:private) (a::private) (bridge:public)) (in-package :blocked) (defun blocked () (public))")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("client.lisp", "CLIENT::caller"),
    })?;
    for expression in [
        "public",
        "private",
        "a:public",
        "a::private",
        "bridge:public",
    ] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.expression == expression
                    && e.resolution == "resolved"
                    && e.target.as_ref().is_some_and(|t| t.file == "api.lisp")),
            "{expression}: {:?}",
            rows.rows
        );
    }
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "a:private" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    let blocked = p.query(Query::Explain {
        selector: selector("client.lisp", "BLOCKED::blocked"),
    })?;
    assert!(
        relationships(&blocked)
            .iter()
            .any(|e| e.expression == "public" && e.resolution == "unresolved"),
        "{:?}",
        blocked.rows
    );
    Ok(())
}

#[test]
fn lisp_package_export_changes_invalidate_unchanged_callers() -> TestResult {
    let p = Project::new()?;
    p.write("package.lisp", "(defpackage :api (:export :target))")?;
    p.write("api.lisp", "(in-package :api) (defun target () nil)")?;
    p.write("client.lisp", "(defun caller () (api:target))")?;
    p.build(false)?;
    let query = Query::Deps {
        selector: selector("client.lisp", "caller"),
    };
    assert_eq!(relationships(&p.query(query.clone())?).len(), 1);
    p.write("package.lisp", "(defpackage :api)")?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    assert!(relationships(&p.query(query.clone())?).is_empty());
    p.build(true)?;
    assert!(relationships(&p.query(query)?).is_empty());
    Ok(())
}

#[test]
fn lisp_package_string_case_and_escaped_symbol_exports_are_exact() -> TestResult {
    let p = Project::new()?;
    p.write("api.lisp",r#"(defpackage "Mixed" (:export "target")) (in-package "Mixed") (defun |target| () nil) (defun TARGET () nil)"#)?;
    p.write(
        "main.lisp",
        r"(defun caller () (|Mixed|:|target|) (mixed:target) (|Mixed|:TARGET))",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.lisp", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "|Mixed|:|target|"
                && e.resolution == "resolved"
                && e.target.as_ref().is_some_and(|t| t.name == "|target|")),
        "{:?}",
        rows.rows
    );
    for name in ["mixed:target", "|Mixed|:TARGET"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.expression == name && e.resolution == "unresolved"),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn lisp_conflicting_used_packages_are_ambiguous_not_a_global_name_guess() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp","(defpackage :left (:export :target)) (defpackage :right (:export :target)) (defpackage :client (:use :left :right)) (in-package :left) (defun target () nil) (in-package :right) (defun target () nil) (in-package :client) (defun caller () (target))")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.lisp", "CLIENT::caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .filter(|e| e.expression == "target")
            .all(|e| e.resolution == "ambiguous"),
        "{:?}",
        rows.rows
    );
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.expression == "target" && e.target.is_some())
            .count(),
        2
    );
    Ok(())
}
