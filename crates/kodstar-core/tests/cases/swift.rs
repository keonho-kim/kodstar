// Purpose: Swift 반환·property·factory와 같은 모듈 선언 우선순위 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn swift_constructor_local_returns_and_parameters_establish_receivers() -> TestResult {
    let p = Project::new()?;
    p.write("Main.swift","struct Worker {\n func run() {}\n}\nfunc make() -> Worker { Worker() }\nfunc caller(input: Worker){ let value=make(); value.run(); input.run(); make().run() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.swift", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Worker::run"))
            .map(|e| e.count)
            .sum::<u64>(),
        3,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn swift_typed_fields_and_factory_properties_keep_the_actual_return_type() -> TestResult {
    let p = Project::new()?;
    p.write("Main.swift","class Worker {\n func run() {}\n}\nclass Factory {\n static func make() -> Worker { Worker() }\n}\nclass Owner {\n var worker: Worker\n var inferred=Factory.make()\n static let shared=Owner()\n func caller(){ self.worker.run(); self.inferred.run() }\n}\nfunc use(){ let owner=Owner.shared; owner.caller() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.swift", "Owner::caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Worker::run"))
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    let singleton = p.query(Query::Deps {
        selector: selector("Main.swift", "use"),
    })?;
    assert!(
        relationships(&singleton).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Owner::caller")),
        "{:?}",
        singleton.rows
    );
    Ok(())
}

#[test]
fn swift_same_module_types_win_over_external_wildcard_imports() -> TestResult {
    let p = Project::new()?;
    p.write("Worker.swift", "class Worker {\n func run() {}\n}\n")?;
    p.write(
        "Main.swift",
        "import Foundation\nfunc caller(value: Worker){ value.run() }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.swift", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "resolved"
            && e.target
                .as_ref()
                .is_some_and(|t| t.file == "Worker.swift" && t.name == "run")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn swift_optional_receivers_preserve_declared_types_without_flattening_collections() -> TestResult {
    let p = Project::new()?;
    p.write("Main.swift","class Worker {\n func run() {}\n}\nfunc caller(value: Worker?, forced: Worker!){ value?.run(); forced!.run() }\nfunc unknown(values: [Worker]){ values.run() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.swift", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Worker::run"))
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    let unknown = p.query(Query::Deps {
        selector: selector("Main.swift", "unknown"),
    })?;
    assert!(!relationships(&unknown).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Worker::run")
    }));
    Ok(())
}

#[test]
fn swift_environment_requires_swiftui_origin_and_never_types_query_as_an_element() -> TestResult {
    for shadow in [false, true] {
        let p = Project::new()?;
        let wrapper = if shadow {
            "struct Environment {}\n"
        } else {
            ""
        };
        p.write("Main.swift", &format!("import SwiftUI\n{wrapper}class Store {{\n func run() {{}}\n}}\nstruct View {{\n @Environment(Store.self) var store\n @Query(Store.self) var items\n func render(){{ store.run(); items.run() }}\n}}\n"))?;
        p.build(false)?;
        let rows = p.query(Query::Deps {
            selector: selector("Main.swift", "View::render"),
        })?;
        assert_eq!(
            relationships(&rows)
                .iter()
                .filter(|e| e.relation == "calls"
                    && e.target
                        .as_ref()
                        .is_some_and(|t| t.qualified_name == "Store::run"))
                .map(|e| e.count)
                .sum::<u64>(),
            u64::from(!shadow),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}
