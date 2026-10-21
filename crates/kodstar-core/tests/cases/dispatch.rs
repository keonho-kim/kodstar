// Purpose: 명시적 상속 멤버·구현 후보와 증분 일관성 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn inherited_member_lookup_uses_explicit_bases_and_respects_overrides() -> TestResult {
    let project = Project::new()?;
    project.write("main.py", "class Base:\n    def target(self): pass\nclass Child(Base):\n    def caller(self): self.target()\n")?;
    project.build(false)?;
    let query = || Query::Deps {
        selector: selector("main.py", "Child::caller"),
    };
    let before = project.query(query())?;
    assert!(relationships(&before).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Base::target")
    }));
    project.write("main.py", "class Base:\n    def target(self): pass\nclass Child(Base):\n    def target(self): pass\n    def caller(self): self.target()\n")?;
    project.build(false)?;
    let after = project.query(query())?;
    assert!(relationships(&after).iter().all(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Child::target")
    }));
    Ok(())
}

#[test]
fn interface_dispatch_candidates_remain_separate_from_the_identified_call_target() -> TestResult {
    let project = Project::new()?;
    project.write("Main.java", "interface Api { void run(); } class Service implements Api { public void run() {} } class Main { void caller(Api value) { value.run(); } }")?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("Main.java", "Main::caller"),
    })?;
    assert!(relationships(&rows).iter().any(|e| {
        e.relation == "calls"
            && e.resolution == "resolved"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Api::run")
    }));
    assert!(relationships(&rows).iter().any(|e| {
        e.relation == "dispatch_candidate"
            && e.resolution == "candidate"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Service::run")
    }));
    let implementation = project.query(Query::Explain {
        selector: selector("Main.java", "Service::run"),
    })?;
    assert!(relationships(&implementation).iter().any(|e| {
        e.relation == "overrides"
            && e.resolution == "candidate"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Api::run")
    }));
    project.write("Main.java", "interface Api { void run(); } class Service { public void run() {} } class Main { void caller(Api value) { value.run(); } }")?;
    project.build(false)?;
    let incremental = project.query(Query::Explain {
        selector: selector("Main.java", "Main::caller"),
    })?;
    assert!(
        !relationships(&incremental)
            .iter()
            .any(|e| e.relation == "dispatch_candidate")
    );
    project.build(true)?;
    let full = project.query(Query::Explain {
        selector: selector("Main.java", "Main::caller"),
    })?;
    assert_eq!(
        relationships(&incremental).len(),
        relationships(&full).len()
    );
    Ok(())
}

#[test]
fn rust_trait_implementation_is_a_type_relation_with_candidate_dispatch() -> TestResult {
    let project = Project::new()?;
    project.write("lib.rs", "trait Api { fn run(&self); } struct Service; impl Api for Service { fn run(&self) {} } fn caller(value: &dyn Api) { value.run(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("lib.rs", "Service"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "implements"
                && e.target.as_ref().is_some_and(|t| t.name == "Api"))
    );
    let calls = project.query(Query::Explain {
        selector: selector("lib.rs", "caller"),
    })?;
    assert!(relationships(&calls).iter().any(|e| {
        e.relation == "dispatch_candidate"
            && e.resolution == "candidate"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Service::run")
    }));
    Ok(())
}
