// Purpose: 언어별 특수 선언·상속·모듈 의미 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn explicit_inheritance_lists_are_not_lost_in_grammar_wrappers() -> TestResult {
    for (file, source, derived, base) in [
        (
            "main.py",
            "class Base: pass\nclass Derived(Base): pass\n",
            "Derived",
            "Base",
        ),
        (
            "Main.java",
            "interface Base {} class Derived implements Base {}",
            "Derived",
            "Base",
        ),
        (
            "main.cpp",
            "struct Base {}; class Derived: public Base {};",
            "Derived",
            "Base",
        ),
        (
            "main.cs",
            "interface Base {} class Derived: Base {}",
            "Derived",
            "Base",
        ),
        (
            "main.php",
            "<?php interface Base {} class Derived implements Base {}",
            "Derived",
            "Base",
        ),
        (
            "main.kt",
            "interface Base\nclass Derived: Base\n",
            "Derived",
            "Base",
        ),
        (
            "main.swift",
            "protocol Base {}\nclass Derived: Base {}\n",
            "Derived",
            "Base",
        ),
        (
            "main.rb",
            "class Base\nend\nclass Derived < Base\nend\n",
            "Derived",
            "Base",
        ),
        (
            "main.scala",
            "trait Base {}\nclass Derived extends Base {}\n",
            "Derived",
            "Base",
        ),
    ] {
        let project = Project::new()?;
        project.write(file, source)?;
        project.build(false)?;
        let rows = project.query(Query::Deps {
            selector: selector(file, derived),
        })?;
        assert!(
            relationships(&rows).iter().any(|e| matches!(
                e.relation.as_str(),
                "inherits" | "implements"
            ) && e
                .target
                .as_ref()
                .is_some_and(|t| t.name == base)),
            "{file}: {:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn lisp_classes_generics_methods_and_macros_preserve_declaration_kind() -> TestResult {
    let project = Project::new()?;
    project.write("main.lisp", "(in-package :app) (defclass service () ()) (defgeneric target (s)) (defmethod target ((s service)) (run s)) (defmacro run (x) x)")?;
    project.build(false)?;
    let rows = project.query(Query::Symbols {
        file: "main.lisp".into(),
    })?;
    for (name, kind) in [("service", "class"), ("run", "macro")] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r,QueryRow::Symbol(s) if s.name==name && s.kind==kind)),
            "{:?}",
            rows.rows
        );
    }
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="target" && s.kind=="method"))
    );
    Ok(())
}

#[test]
fn beam_remote_calls_keep_module_and_arity_and_elixir_aliases() -> TestResult {
    for (file, source, dep, definition, caller) in [
        (
            "main.erl",
            "-module(main).\ncaller() -> api:target(ok).\n",
            "api.erl",
            "-module(api).\n-export([target/1]).\ntarget(X) -> X.\n",
            "caller/0",
        ),
        (
            "main.ex",
            "defmodule App.Main do\n alias App.Api, as: Api\n def caller(), do: Api.target()\nend\n",
            "api.ex",
            "defmodule App.Api do\n def target(), do: :ok\nend\n",
            "caller",
        ),
    ] {
        let project = Project::new()?;
        project.write(file, source)?;
        project.write(dep, definition)?;
        project.build(false)?;
        let rows = project.query(Query::Deps {
            selector: selector(file, caller),
        })?;
        assert_eq!(
            relationships(&rows)
                .iter()
                .filter(
                    |e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.file == dep)
                )
                .count(),
            1,
            "{file}: {:?}",
            rows.rows
        );
    }
    Ok(())
}
