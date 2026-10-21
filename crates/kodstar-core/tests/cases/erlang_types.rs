// Purpose: Erlang type·opaque·macro·명세 문맥과 export 이름 공간 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/parse/erlang/types.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn erlang_static_types_macros_and_specs_keep_separate_name_spaces() -> TestResult {
    let p = Project::new()?;
    p.write("sample.erl", "-module(sample).\n-type item(A) :: {A, integer()}.\n-opaque token() :: binary().\n-define(COUNT, 1).\n-define(ADD(X,Y), X + Y).\n-spec run(item(integer())) -> token().\nrun(X) -> ?ADD(X,?COUNT).\nitem(X) -> X.\n")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "sample.erl".into(),
    })?;
    for (name, kind) in [
        ("item/1", "type"),
        ("token/0", "type"),
        ("COUNT", "macro"),
        ("ADD/2", "macro"),
    ] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == name && s.kind == kind)),
            "{:?}",
            rows.rows
        );
    }
    let rows = p.query(Query::Explain {
        selector: selector("sample.erl", "run/1"),
    })?;
    for target in ["COUNT", "ADD/2"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "references"
                    && e.target
                        .as_ref()
                        .is_some_and(|t| t.name == target && t.kind == "macro")),
            "{:?}",
            rows.rows
        );
    }
    let rows = p.query(Query::Explain {
        selector: selector("sample.erl", "sample.erl"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "uses_type"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.name == "item/1" && t.kind == "type")),
        "{:?}",
        rows.rows
    );
    assert!(!relationships(&rows).iter().any(|e| e.relation == "calls"));
    Ok(())
}

#[test]
fn exported_remote_types_do_not_resolve_to_same_named_functions_or_private_types() -> TestResult {
    let p = Project::new()?;
    p.write("api.erl", "-module(api).\n-export([item/1, hidden/0]).\n-export_type([item/1]).\n-type item(A) :: A.\n-type hidden() :: integer().\nitem(X) -> X.\nhidden() -> ok.\n")?;
    p.write("caller.erl", "-module(caller).\n-spec run(api:item(integer())) -> api:hidden().\nrun(X) -> api:item(X), api:hidden().\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("caller.erl", "caller.erl"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "uses_type"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.name == "item/1" && t.kind == "type")),
        "{:?}",
        rows.rows
    );
    assert!(
        !relationships(&rows).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.name == "hidden/0" || t.kind == "function")),
        "{:?}",
        rows.rows
    );
    let rows = p.query(Query::Deps {
        selector: selector("caller.erl", "run/1"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.kind == "function"))
            .count(),
        2,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn included_macros_are_references_without_evaluating_their_replacement() -> TestResult {
    let p = Project::new()?;
    p.write(
        "defs.hrl",
        "-define(COUNT, 1).\n-define(ADD(X,Y), X + Y).\n",
    )?;
    p.write(
        "sample.erl",
        "-module(sample).\n-include(\"defs.hrl\").\nrun(X) -> ?ADD(X,?COUNT).\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("sample.erl", "run/1"),
    })?;
    for target in ["COUNT", "ADD/2"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "references"
                    && e.target
                        .as_ref()
                        .is_some_and(|t| t.name == target && t.file == "defs.hrl")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}
