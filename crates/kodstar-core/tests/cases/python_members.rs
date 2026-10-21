// Purpose: Python property·constructor·명시적 반환 receiver 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn python_constructor_and_annotated_return_results_have_proven_receivers() -> TestResult {
    let p = Project::new()?;
    p.write("members.py","class Client:\n    def work(self): pass\ndef factory() -> Client:\n    return Client()\ndef caller():\n    direct=Client()\n    returned=factory()\n    direct.work()\n    returned.work()\n    factory().work()\n    Client().work()\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("members.py", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Client::work"))
            .map(|e| e.count)
            .sum::<u64>(),
        4,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn python_property_reads_connect_getters_and_return_types_but_stores_do_not() -> TestResult {
    let p = Project::new()?;
    p.write("members.py","class Client:\n    def work(self): pass\nclass Service:\n    @property\n    def client(self) -> Client:\n        return Client()\n    def read(self):\n        item=self.client\n        self.client.work()\n    def write(self):\n        self.client=None\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("members.py", "Service::read"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Service::client")),
        "{:?}",
        rows.rows
    );
    assert!(relationships(&rows).iter().any(|e| {
        e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Client::work")
    }));
    let writes = p.query(Query::Deps {
        selector: selector("members.py", "Service::write"),
    })?;
    assert!(!relationships(&writes).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Service::client")
    }));
    Ok(())
}

#[test]
fn python_c3_order_and_super_keep_the_first_valid_definition() -> TestResult {
    let p = Project::new()?;
    p.write("members.py","class Root:\n    def run(self): pass\nclass Left(Root): pass\nclass Right(Root):\n    def run(self): pass\nclass Child(Left,Right):\n    def own(self):\n        self.run()\n    def run(self):\n        super().run()\ndef caller(value: Child):\n    value.run()\n")?;
    p.build(false)?;
    for (caller, target) in [
        ("Child::own", "Child::run"),
        ("Child::run", "Right::run"),
        ("caller", "Child::run"),
    ] {
        let rows = p.query(Query::Deps {
            selector: selector("members.py", caller),
        })?;
        let calls: Vec<_> = relationships(&rows)
            .into_iter()
            .filter(|e| e.relation == "calls")
            .collect();
        assert_eq!(calls.len(), 1, "{caller}: {:?}", rows.rows);
        assert!(
            calls[0]
                .target
                .as_ref()
                .is_some_and(|t| t.qualified_name == target),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn python_shadowed_property_decorator_does_not_claim_builtin_getter_behavior() -> TestResult {
    let p = Project::new()?;
    p.write("members.py","def property(fn): return fn\nclass Client:\n    @property\n    def value(self): pass\ndef caller(client: Client):\n    return client.value\n")?;
    p.build(false)?;
    assert!(
        !relationships(&p.query(Query::Deps {
            selector: selector("members.py", "caller")
        })?)
        .iter()
        .any(|e| e.relation == "references"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Client::value"))
    );
    Ok(())
}
