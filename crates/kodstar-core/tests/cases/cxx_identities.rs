// Purpose: include 근거의 C/C++ forward·free function·static 정체성 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn c_and_cpp_free_declarations_match_a_definition_with_the_same_signature() -> TestResult {
    for extension in ["c", "cpp"] {
        let p = Project::new()?;
        let file = format!("main.{extension}");
        p.write(&file,"int target(int input); int target(int value){return value;} int caller(){return target(1);}")?;
        p.build(false)?;
        let rows = p.query(Query::Deps {
            selector: selector(&file, "caller"),
        })?;
        let calls = relationships(&rows)
            .into_iter()
            .filter(|e| e.relation == "calls")
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1, "{:?}", rows.rows);
        assert_eq!(calls[0].resolution, "resolved");
        assert_eq!(
            calls[0].target.as_ref().ok_or("target")?.definitions.len(),
            2
        );
    }
    Ok(())
}

#[test]
fn cpp_free_definitions_require_include_proof_and_survive_implementation_deletion() -> TestResult {
    let p = Project::new()?;
    p.write("api.hpp", "int target(int input);")?;
    p.write(
        "impl.cpp",
        "#include \"api.hpp\"\nint target(int value){return value;}",
    )?;
    p.write("unrelated.cpp", "int target(int value){return value;}")?;
    p.write(
        "main.cpp",
        "#include \"api.hpp\"\nint caller(){return target(1);}",
    )?;
    p.build(false)?;
    let query = || Query::Deps {
        selector: selector("main.cpp", "caller"),
    };
    let rows = p.query(query())?;
    let target = relationships(&rows)
        .into_iter()
        .find(|e| e.relation == "calls" && e.resolution == "resolved")
        .and_then(|e| e.target.as_ref())
        .ok_or_else(|| format!("{:?}", rows.rows))?;
    assert_eq!(target.definitions.len(), 2);
    let key = target.logical_key.clone();
    std::fs::remove_file(p.root.join("impl.cpp"))?;
    p.build(false)?;
    let incremental = p.query(query())?;
    assert!(relationships(&incremental).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.logical_key == key && t.definitions.is_empty())
    }));
    p.build(true)?;
    assert!(
        relationships(&p.query(query())?)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.logical_key == key))
    );
    Ok(())
}

#[test]
fn cpp_forward_types_share_members_only_with_reachable_definitions() -> TestResult {
    let p = Project::new()?;
    p.write("worker.hpp", "class Worker { public: void run() {} };")?;
    p.write(
        "main.cpp",
        "class Worker;\n#include \"worker.hpp\"\nvoid caller(Worker &worker){ worker.run(); }",
    )?;
    p.write("unrelated.cpp", "class Worker { public: void other() {} };")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "resolved"
            && e.target
                .as_ref()
                .is_some_and(|t| t.name == "run" && t.file == "worker.hpp")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn c_static_functions_in_different_files_are_distinct_and_local_prototypes_merge() -> TestResult {
    let p = Project::new()?;
    for file in ["one.c", "two.c"] {
        p.write(file,"static int target(int x); static int target(int x){return x;} int caller(){return target(1);}")?;
    }
    p.build(false)?;
    let mut keys = Vec::new();
    for file in ["one.c", "two.c"] {
        let rows = p.query(Query::Deps {
            selector: selector(file, "caller"),
        })?;
        let target = relationships(&rows)
            .into_iter()
            .find(|e| e.relation == "calls" && e.resolution == "resolved")
            .and_then(|e| e.target.as_ref())
            .ok_or_else(|| format!("{:?}", rows.rows))?;
        assert_eq!(target.file, file);
        assert_eq!(target.definitions.len(), 2);
        keys.push(target.logical_key.clone());
    }
    assert_ne!(keys[0], keys[1]);
    Ok(())
}

#[test]
fn cpp_free_conflicting_definitions_remain_ambiguous_instead_of_picking_one() -> TestResult {
    let p = Project::new()?;
    p.write("api.hpp", "int target(int input);")?;
    for file in ["one.cpp", "two.cpp"] {
        p.write(
            file,
            "#include \"api.hpp\"\nint target(int input){return input;}",
        )?;
    }
    p.write(
        "main.cpp",
        "#include \"api.hpp\"\nint caller(){return target(1);}",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.cpp", "caller"),
    })?;
    let calls = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 3, "{:?}", rows.rows);
    assert!(calls.iter().all(|e| e.resolution == "ambiguous"));
    Ok(())
}

#[test]
fn cpp_forward_conflicting_bodies_do_not_establish_one_receiver_owner() -> TestResult {
    let p = Project::new()?;
    p.write("one.hpp", "class Worker { public: void run() {} };")?;
    p.write("two.hpp", "class Worker { public: void run() {} };")?;
    p.write("main.cpp","class Worker;\n#include \"one.hpp\"\n#include \"two.hpp\"\nvoid caller(Worker &value){value.run();}")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.cpp", "caller"),
    })?;
    let calls = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2, "{:?}", rows.rows);
    assert!(calls.iter().all(|e| e.resolution == "ambiguous"));
    Ok(())
}

#[test]
fn cpp_namespace_free_declarations_follow_indirect_cyclic_includes() -> TestResult {
    let p = Project::new()?;
    p.write(
        "api.hpp",
        "#include \"bridge.hpp\"\nnamespace App {int target(int input);}",
    )?;
    p.write("bridge.hpp", "#include \"api.hpp\"\n")?;
    p.write(
        "impl.cpp",
        "#include \"bridge.hpp\"\nint App::target(int input){return input;}",
    )?;
    p.write(
        "main.cpp",
        "#include \"bridge.hpp\"\nint caller(){return App::target(1);}",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "resolved"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "App::target" && t.definitions.len() == 2)),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn cpp_incompatible_return_declarations_are_preserved_without_a_merge() -> TestResult {
    let p = Project::new()?;
    p.write("main.cpp","int target(int input); double target(int value){return value;} int caller(){return target(1);}")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.cpp", "caller"),
    })?;
    let calls = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2, "{:?}", rows.rows);
    assert!(calls.iter().all(|e| e.resolution == "ambiguous"
        && e.target.as_ref().is_some_and(|t| t.definitions.is_empty())));
    Ok(())
}

#[test]
fn cpp_unevaluated_include_conditions_do_not_establish_firm_calls() -> TestResult {
    let p = Project::new()?;
    p.write("api.hpp", "int target(int input);")?;
    p.write(
        "main.cpp",
        "#if FEATURE\n#include \"api.hpp\"\n#endif\nint caller(){return target(1);}",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.cpp", "caller"),
    })?;
    let calls = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1, "{:?}", rows.rows);
    assert_eq!(calls[0].resolution, "candidate", "{:?}", rows.rows);
    Ok(())
}
