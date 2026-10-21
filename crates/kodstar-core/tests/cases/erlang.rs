// Purpose: Erlang name/arity 절·공개 함수와 callback 참조 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn erlang_clauses_share_identity_and_exports_limit_remote_visibility() -> TestResult {
    let p = Project::new()?;
    p.write("sample.erl","-module(sample).\n-export([run/1]).\nrun(a) -> helper();\nrun(b) -> helper().\nhelper() -> ok.\n")?;
    p.write(
        "caller.erl",
        "-module(caller).\ncall() -> sample:run(a), sample:helper().\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("caller.erl", "call/0"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(
        calls.iter().filter(|e| e.resolution == "resolved").count(),
        1,
        "{:?}",
        rows.rows
    );
    let target = calls
        .iter()
        .find_map(|e| e.target.as_ref().filter(|t| t.name == "run/1"))
        .ok_or("run")?;
    assert_eq!(
        serde_json::to_value(target)?["definitions"]
            .as_array()
            .ok_or("definitions")?
            .len(),
        2
    );
    let local = p.query(Query::Deps {
        selector: selector("sample.erl", "run/1"),
    })?;
    assert_eq!(
        relationships(&local)
            .iter()
            .filter(|e| e.relation == "calls")
            .map(|e| e.count)
            .sum::<u64>(),
        2
    );
    Ok(())
}

#[test]
fn erlang_imports_and_function_values_preserve_exact_arity() -> TestResult {
    let p = Project::new()?;
    p.write(
        "sample.erl",
        "-module(sample).\n-export([run/1]).\nrun(X) -> X.\nrun() -> ok.\n",
    )?;
    p.write(
        "caller.erl",
        "-module(caller).\n-import(sample,[run / 1]).\ncall() -> run(1), fun sample:run/1.\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("caller.erl", "call/0"),
    })?;
    for relation in ["calls", "references"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == relation
                    && e.target.as_ref().is_some_and(|t| t.name == "run/1")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn erlang_included_records_and_clause_removal_preserve_other_definitions() -> TestResult {
    let p = Project::new()?;
    p.write("types.hrl", "-record(user,{id,name}).")?;
    p.write("sample.erl","-module(sample).\n-include(\"types.hrl\").\n-export([run/1]).\nrun(a) -> #user{id=1};\nrun(b) -> #user{id=2}.\n")?;
    p.build(false)?;
    let query = || Query::Explain {
        selector: selector("sample.erl", "run/1"),
    };
    let before = p.query(query())?;
    assert!(
        relationships(&before)
            .iter()
            .any(|e| e.relation == "uses_type"
                && e.target.as_ref().is_some_and(|t| t.name == "user")),
        "{:?}",
        before.rows
    );
    let key = before
        .rows
        .iter()
        .find_map(|r| {
            if let QueryRow::Symbol(s) = r {
                Some(s.logical_key.clone())
            } else {
                None
            }
        })
        .ok_or("symbol")?;
    p.write(
        "sample.erl",
        "-module(sample).\n-include(\"types.hrl\").\n-export([run/1]).\nrun(b) -> #user{id=2}.\n",
    )?;
    p.build(false)?;
    let incremental = p.query(query())?;
    let after = incremental
        .rows
        .iter()
        .find_map(|r| {
            if let QueryRow::Symbol(s) = r {
                Some(s.logical_key.clone())
            } else {
                None
            }
        })
        .ok_or("symbol")?;
    assert_eq!(key, after);
    p.build(true)?;
    let full = p.query(query())?;
    assert!(
        full.rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.logical_key==key))
    );
    Ok(())
}

#[test]
fn erlang_function_values_publish_references_and_candidate_indirect_calls() -> TestResult {
    let p = Project::new()?;
    p.write(
        "api.erl",
        "-module(api).\n-export([target/1]).\ntarget(X) -> X.\n",
    )?;
    p.write(
        "caller.erl",
        "-module(caller).\nlocal(X) -> X.\ncall() -> [fun local/1, fun api:target/1].\n",
    )?;
    p.build(false)?;
    let mut rows = Collected::default();
    p.engine.query(
        &p.options(),
        &Query::Explain {
            selector: selector("caller.erl", "call/0"),
        },
        &QueryOptions {
            refresh: Refresh::None,
            include_candidates: true,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut rows,
    )?;
    for target in ["local/1", "target/1"] {
        for relation in ["references", "indirect_call"] {
            assert!(
                relationships(&rows).iter().any(|e| e.relation == relation
                    && e.target.as_ref().is_some_and(|t| t.name == target)
                    && e.resolution
                        == if relation == "references" {
                            "resolved"
                        } else {
                            "candidate"
                        }),
                "{:?}",
                rows.rows
            );
        }
    }
    assert!(!relationships(&rows).iter().any(|e| e.relation == "calls"));
    Ok(())
}
