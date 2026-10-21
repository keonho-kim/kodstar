// Purpose: Python scope 지시·구조 분해·lambda shadowing 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn python_global_and_nonlocal_choose_the_declared_lexical_owner() -> TestResult {
    let p = Project::new()?;
    p.write("scope.py", "def target(): pass\ndef outer():\n    def target(): pass\n    def global_call():\n        global target\n        target()\n    def enclosing_call():\n        nonlocal target\n        target()\n")?;
    p.build(false)?;
    for (caller, target) in [
        ("outer::global_call", "target"),
        ("outer::enclosing_call", "outer::target"),
    ] {
        let rows = p.query(Query::Deps {
            selector: selector("scope.py", caller),
        })?;
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == target)),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn python_destructuring_loops_and_lambda_parameters_shadow_outer_functions() -> TestResult {
    let p = Project::new()?;
    p.write("scope.py","def target(): pass\ndef packed(values):\n    target, other = values\n    target()\ndef loop(values):\n    for target in values:\n        target()\ndef outer():\n    fn = lambda target: target()\n")?;
    p.build(false)?;
    for caller in ["packed", "loop", "outer"] {
        let rows = p.query(Query::Deps {
            selector: selector("scope.py", caller),
        })?;
        assert!(
            !relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "target")),
            "{caller}: {:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn python_future_local_assignment_does_not_resolve_a_call_to_its_future_value() -> TestResult {
    let p = Project::new()?;
    p.write(
        "scope.py",
        "def target(): pass\ndef caller():\n    local()\n    local=target\n",
    )?;
    p.build(false)?;
    assert!(
        relationships(&p.query(Query::Deps {
            selector: selector("scope.py", "caller")
        })?)
        .iter()
        .all(|edge| edge.relation != "calls")
    );
    Ok(())
}
