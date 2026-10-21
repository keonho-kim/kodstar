// Purpose: 외부 타입의 멤버 경로와 재수출 분기별 후보 격리 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn an_external_producer_alternative_keeps_an_internal_return_receiver_uncertain() -> TestResult {
    let p = Project::new()?;
    p.write("Types.cs","namespace Inside { class Item { public void Run() {} } class Factory { public static Item Make() { return new Item(); } } }")?;
    p.write("Main.cs","using static Inside.Factory; using static Outside.Factory; class Main { void Caller(){ Make().Run(); } }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.cs", "Main::Caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "candidate"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Inside::Item::Run")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn external_typed_receivers_keep_the_called_member_in_the_external_key() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.py",
        "from outside import Service\ndef caller(value: Service):\n    value.run()\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.py", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "candidate"
            && e.external
                .as_ref()
                .is_some_and(|t| t.contains("Service::run"))),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn member_selection_does_not_rewrite_another_reexport_branch_external_target() -> TestResult {
    let p = Project::new()?;
    p.write("lib.rs","mod local { pub struct Thing {} impl Thing { pub fn target() {} } } mod facade { pub use outside::target; pub use crate::local::Thing::target; } use crate::facade::target as invoke; fn caller(){ invoke(); }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("lib.rs", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.external
                .as_ref()
                .is_some_and(|t| t.ends_with("outside::target") && !t.ends_with("target::target"))),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "local::Thing::target")),
        "{:?}",
        rows.rows
    );
    Ok(())
}
