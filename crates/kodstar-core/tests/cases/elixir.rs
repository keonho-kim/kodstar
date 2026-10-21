// Purpose: Elixir 중첩 module·alias·선택 import·arity와 절 병합 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn elixir_nested_modules_and_group_aliases_keep_each_module_owner() -> TestResult {
    let p = Project::new()?;
    p.write("modules.ex","defmodule Outer do\n def root(), do: :ok\n defmodule Child do\n  def run(), do: Outer.root()\n end\nend\ndefmodule Caller do\n alias Outer.{Child, Other}\n def call(), do: Child.run()\nend\n")?;
    p.build(false)?;
    for (caller, target) in [
        ("Caller::call", "Outer::Child::run"),
        ("Outer::Child::run", "Outer::root"),
    ] {
        let rows = p.query(Query::Deps {
            selector: selector("modules.ex", caller),
        })?;
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == target)),
            "{caller}: {:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn elixir_arity_selective_imports_and_private_functions_stay_distinct() -> TestResult {
    let p = Project::new()?;
    p.write("api.ex","defmodule Api do\n def run(), do: :ok\n def run(value), do: value\n defp secret(), do: :ok\nend\n")?;
    p.write("caller.ex","defmodule Caller do\n import Api, only: [run: 0]\n def call() do\n  run()\n  run(1)\n  Api.secret()\n end\nend\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("caller.ex", "Caller::call"),
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
    assert!(
        calls
            .iter()
            .any(|e| e.resolution == "resolved"
                && e.target.as_ref().is_some_and(|t| t.name == "run"))
    );
    assert!(
        !calls
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "secret"))
    );
    Ok(())
}

#[test]
fn elixir_nested_modules_do_not_capture_outer_local_functions_or_excluded_arities() -> TestResult {
    let p = Project::new()?;
    p.write("api.ex","defmodule Api do\n def run(), do: :ok\n def run(value), do: value\n defp secret(), do: :ok\nend\ndefmodule Outer do\n def local(), do: :ok\n defmodule Child do\n  import Api, except: [run: 0]\n  def call() do\n   local()\n   run()\n   run(1)\n   Api.secret()\n  end\n end\nend\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("api.ex", "Outer::Child::call"),
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
    assert!(
        calls
            .iter()
            .any(|e| e.expression == "run/1" && e.resolution == "resolved")
    );
    Ok(())
}

#[test]
fn elixir_guarded_clauses_defaults_and_pipelines_use_the_actual_arity() -> TestResult {
    let p = Project::new()?;
    p.write("api.ex","defmodule Api do\n def run(value) when is_integer(value), do: value\n def run(value), do: value\n def choice(value \\\\ 1), do: value\n def call() do\n  1 |> run()\n  choice()\n end\nend\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("api.ex", "Api::call"),
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
    let run = p.query(Query::Explain {
        selector: selector("api.ex", "Api::run"),
    })?;
    assert!(
        run.rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.definitions.len()==2))
    );
    Ok(())
}

#[test]
fn elixir_protocols_and_implementations_own_their_functions() -> TestResult {
    let p = Project::new()?;
    p.write("protocol.ex", "defprotocol Size do\n def size(data)\nend\n")?;
    p.write("implementation.ex", "defimpl Size, for: String do\n def size(value), do: helper(value)\n defp helper(value), do: value\nend\ndefmodule Other do\n def size(), do: :other\nend\n")?;
    p.build(false)?;
    let inventory = p.query(Query::Symbols {
        file: "implementation.ex".into(),
    })?;
    assert!(
        inventory
            .rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.qualified_name == "Size::String")),
        "{:?}",
        inventory.rows
    );
    let rows = p.query(Query::Explain {
        selector: selector("implementation.ex", "Size::String"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "implements"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Size" && t.file == "protocol.ex")),
        "{:?}",
        rows.rows
    );
    let rows = p.query(Query::Deps {
        selector: selector("implementation.ex", "Size::String::size"),
    })?;
    assert!(relationships(&rows).iter().any(|e| {
        e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Size::String::helper")
    }));
    let rows = p.query(Query::Symbols {
        file: "protocol.ex".into(),
    })?;
    assert!(rows.rows.iter().any(|r| matches!(r, QueryRow::Symbol(s) if s.qualified_name == "Size::size" && s.owner.is_some())));
    Ok(())
}

#[test]
fn elixir_captures_keep_arity_and_do_not_become_zero_argument_calls() -> TestResult {
    let p = Project::new()?;
    p.write("api.ex", "defmodule Api do\n def target(value), do: value\n def target(), do: :zero\n defp secret(value), do: value\nend\n")?;
    p.write("caller.ex", "defmodule Caller do\n alias Api, as: A\n def local(value), do: value\n def local(), do: :zero\n def call(), do: [&A.target/1, &local/1, &Api.secret/1]\nend\n")?;
    p.build(false)?;
    let mut rows = Collected::default();
    p.engine.query(
        &p.options(),
        &Query::Explain {
            selector: selector("caller.ex", "Caller::call"),
        },
        &QueryOptions {
            refresh: Refresh::None,
            include_candidates: true,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut rows,
    )?;
    let edges = relationships(&rows);
    assert!(
        !edges.iter().any(|e| e.relation == "calls"),
        "{:?}",
        rows.rows
    );
    for expression in ["A.target/1", "local/1"] {
        for relation in ["references", "indirect_call"] {
            assert!(
                edges.iter().any(|e| e.expression == expression
                    && e.relation == relation
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
    assert!(
        !edges
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "secret"))
    );
    Ok(())
}
