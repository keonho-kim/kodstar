// Purpose: Python comprehension의 독립 scope·첫 iterable·바깥 walrus 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn python_comprehension_targets_do_not_leak_and_the_first_iterable_uses_outer_scope() -> TestResult
{
    for expression in [
        "[target() for target in target()]",
        "{target() for target in target()}",
        "{target(): 1 for target in target()}",
        "(target() for target in target())",
    ] {
        let p = Project::new()?;
        p.write(
            "main.py",
            &format!(
                "def target(): return []\ndef caller():\n    values={expression}\n    target()\n"
            ),
        )?;
        p.build(false)?;
        let rows = p.query(Query::Deps {
            selector: selector("main.py", "caller"),
        })?;
        assert_eq!(
            relationships(&rows)
                .iter()
                .filter(|e| e.relation == "calls"
                    && e.target.as_ref().is_some_and(|t| t.name == "target"))
                .map(|e| e.count)
                .sum::<u64>(),
            2,
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn python_nested_comprehensions_keep_their_enclosing_iteration_binding() -> TestResult {
    let p = Project::new()?;
    p.write("main.py","def target(): return []\ndef source(): return []\ndef caller():\n    values=[[target() for inner in target()] for target in source()]\n    target()\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.py", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(
                |e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == "target")
            )
            .map(|e| e.count)
            .sum::<u64>(),
        1,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn python_comprehension_walrus_binds_the_enclosing_function() -> TestResult {
    let p = Project::new()?;
    p.write("main.py","def saved(): pass\ndef caller(items):\n    values=[(saved := item) for item in items]\n    saved()\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.py", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.expression == "saved"
            && e.relation == "calls"
            && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn python_class_comprehension_body_skips_class_locals_but_its_iterable_can_use_them() -> TestResult
{
    let p = Project::new()?;
    p.write("main.py","def choose(): return 1\ndef make_source(): return []\nclass Box:\n    choose=make_source\n    values=[choose() for item in choose()]\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.py", "Box"),
    })?;
    for name in ["choose", "make_source"] {
        assert_eq!(
            relationships(&rows)
                .iter()
                .filter(
                    |e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == name)
                )
                .map(|e| e.count)
                .sum::<u64>(),
            1,
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}
