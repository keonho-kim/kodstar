// Purpose: C++ 분리 메서드·선언/정의·서명 구별 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn cpp_method_declarations_and_one_definition_share_owner_and_locations() -> TestResult {
    let p = Project::new()?;
    p.write(
        "worker.hpp",
        "namespace App { struct Worker { void target(int input); }; }",
    )?;
    p.write("worker.cpp","#include \"worker.hpp\"\nvoid App::Worker::target(int value) {}\nvoid caller(App::Worker &worker) { worker.target(1); }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("worker.cpp", "caller"),
    })?;
    let target = relationships(&rows)
        .into_iter()
        .find(|e| e.relation == "calls" && e.resolution == "resolved")
        .and_then(|e| e.target.as_ref())
        .ok_or_else(|| format!("missing call: {:?}", rows.rows))?;
    assert_eq!(target.qualified_name, "App::Worker::target");
    assert_eq!(target.definitions.len(), 2);
    let definition = p.query(Query::Explain {
        selector: selector("worker.cpp", "App::Worker::target"),
    })?;
    let declaration = p.query(Query::Explain {
        selector: selector("worker.hpp", "App::Worker::target"),
    })?;
    let symbol = |rows: &Collected| {
        rows.rows.iter().find_map(|r| {
            if let QueryRow::Symbol(s) = r {
                Some(s.id)
            } else {
                None
            }
        })
    };
    assert_eq!(symbol(&definition), symbol(&declaration));
    Ok(())
}

#[test]
fn cpp_distinct_parameter_and_const_signatures_are_not_merged() -> TestResult {
    let p = Project::new()?;
    p.write("worker.hpp","struct Worker { void target(int input); void target(double input); void target(int input) const; };")?;
    p.write("worker.cpp","#include \"worker.hpp\"\nvoid Worker::target(int input) {}\nvoid caller(Worker &worker) { worker.target(1); }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("worker.cpp", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 3, "{:?}", rows.rows);
    assert!(calls.iter().all(|e| e.resolution == "ambiguous"));
    assert_eq!(
        calls
            .iter()
            .filter(|e| e.target.as_ref().is_some_and(|t| t.definitions.len() == 2))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn cpp_header_namespaces_do_not_export_their_children_into_the_global_scope() -> TestResult {
    let p = Project::new()?;
    p.write(
        "hidden.hpp",
        "namespace Hidden { void target(); struct Worker {}; }",
    )?;
    p.write(
        "caller.cpp",
        "#include \"hidden.hpp\"\nvoid caller() { target(); Hidden::target(); }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("caller.cpp", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert!(
        calls
            .iter()
            .any(|e| e.expression == "target" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    assert!(
        calls
            .iter()
            .any(|e| e.expression == "Hidden::target" && e.resolution == "resolved")
    );
    Ok(())
}

#[test]
fn cpp_implementation_removal_preserves_the_remaining_member_identity() -> TestResult {
    let p = Project::new()?;
    p.write("worker.hpp", "struct Worker { void target(int input); };")?;
    p.write(
        "00_impl.cpp",
        "#include \"worker.hpp\"\nvoid Worker::target(int renamed) {}",
    )?;
    p.write(
        "caller.cpp",
        "#include \"worker.hpp\"\nvoid caller(Worker &worker) { worker.target(1); }",
    )?;
    p.build(false)?;
    let query = || Query::Deps {
        selector: selector("caller.cpp", "caller"),
    };
    let before = p.query(query())?;
    let target = relationships(&before)
        .into_iter()
        .find(|e| e.relation == "calls")
        .and_then(|e| e.target.as_ref())
        .ok_or("target")?;
    let key = target.logical_key.clone();
    assert_eq!(target.definitions.len(), 2);
    std::fs::remove_file(p.root.join("00_impl.cpp"))?;
    assert_eq!(p.build(false)?.parsed_files, 0);
    let incremental = p.query(query())?;
    assert!(relationships(&incremental).iter().any(|e| {
        e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.logical_key == key && t.definitions.is_empty())
    }));
    p.build(true)?;
    assert!(
        relationships(&p.query(query())?)
            .iter()
            .any(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.logical_key == key))
    );
    Ok(())
}

#[test]
fn cpp_conflicting_definitions_are_retained_as_ambiguous_targets() -> TestResult {
    let p = Project::new()?;
    p.write("worker.hpp", "struct Worker { void target(int input); };")?;
    for file in ["one.cpp", "two.cpp"] {
        p.write(
            file,
            "#include \"worker.hpp\"\nvoid Worker::target(int input) {}",
        )?;
    }
    p.write(
        "caller.cpp",
        "#include \"worker.hpp\"\nvoid caller(Worker &worker) { worker.target(1); }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("caller.cpp", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|e| e.resolution == "ambiguous"));
    Ok(())
}
