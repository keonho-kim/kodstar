// Purpose: C/C++ whole-file guard 정규화와 일반 feature 조건 보존 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn cpp_whole_header_guards_do_not_split_matching_members_and_free_functions() -> TestResult {
    for directive in ["#ifndef API_HPP", "#if !defined(API_HPP)"] {
        let p = Project::new()?;
        p.write("api.hpp",&format!("{directive}\n#define API_HPP\nstruct Worker {{void run();}};\nint target(int input);\n#endif\n"))?;
        p.write(
            "impl.cpp",
            "#include \"api.hpp\"\nvoid Worker::run(){}\nint target(int value){return value;}\n",
        )?;
        p.write(
            "main.cpp",
            "#include \"api.hpp\"\nvoid caller(Worker &value){value.run();target(1);}\n",
        )?;
        p.build(false)?;
        let rows = p.query(Query::Deps {
            selector: selector("main.cpp", "caller"),
        })?;
        for name in ["run", "target"] {
            assert!(
                relationships(&rows).iter().any(|e| e.relation == "calls"
                    && e.resolution == "resolved"
                    && e.target
                        .as_ref()
                        .is_some_and(|t| t.name == name && t.definitions.len() == 2)),
                "{:?}",
                rows.rows
            );
        }
    }
    Ok(())
}

#[test]
fn cpp_feature_conditions_inside_header_guards_remain_part_of_the_identity() -> TestResult {
    let p = Project::new()?;
    p.write("api.hpp","#ifndef API_HPP\n#define API_HPP\nstruct Worker {\n#ifdef FEATURE\nvoid run();\n#endif\n};\n#endif\n")?;
    p.write(
        "impl.cpp",
        "#include \"api.hpp\"\n#ifdef FEATURE\nvoid Worker::run(){}\n#endif\n",
    )?;
    p.write(
        "main.cpp",
        "#include \"api.hpp\"\nvoid caller(Worker &value){value.run();}\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target.as_ref().is_some_and(|t| t.name == "run"
                && t.definitions.len() == 2
                && t.condition.as_ref().is_some_and(|s| s.contains("FEATURE")))),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn cpp_conditions_without_a_matching_guard_definition_keep_distinct_declarations() -> TestResult {
    for body in [
        "#ifndef FEATURE\nint target(int value);\n#endif\n",
        "#ifndef API_HPP\n#define OTHER\nint target(int value);\n#endif\n",
    ] {
        let p = Project::new()?;
        p.write("api.hpp", body)?;
        p.write(
            "impl.cpp",
            "#include \"api.hpp\"\nint target(int value){return value;}\n",
        )?;
        p.write(
            "main.cpp",
            "#include \"api.hpp\"\nint caller(){return target(1);}",
        )?;
        p.build(false)?;
        let rows = p.query(Query::Deps {
            selector: selector("main.cpp", "caller"),
        })?;
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.name == "target"
                    && t.definitions.is_empty()
                    && t.condition.is_some())),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}
