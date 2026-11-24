// Purpose: 내장 Clang의 실제 오버로드·템플릿·매크로 호출 해석 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 native 의미 근거를 확인.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/tests/engine.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn clang_selects_the_actual_overload_and_template_specialization() -> TestResult {
    let p = Project::new()?;
    p.write("main.cpp", "int pick(int n) { return n; }\ndouble pick(double n) { return n; }\ntemplate<typename T> T identity(T n) { return n; }\nint caller() { return pick(identity(1)) + int(pick(2.0)); }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|r| r.relation == "calls")
        .collect();
    assert_eq!(
        calls
            .iter()
            .filter(|r| r.target.as_ref().is_some_and(|t| t.name == "pick"))
            .count(),
        2,
        "{calls:?}"
    );
    assert!(calls.iter().all(|r| r.rule == "clang_semantic_call"));
    Ok(())
}

#[test]
fn clang_indexes_macro_generated_functions_and_their_calls() -> TestResult {
    let p = Project::new()?;
    p.write("main.c", "#define DEFINE(name) int name(int value) { return value; }\nDEFINE(expanded)\nint caller(void) { return expanded(1); }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.c", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.rule == "clang_semantic_call"
                && r.target.as_ref().is_some_and(|t| t.name == "expanded"))
    );
    Ok(())
}

#[test]
fn native_header_changes_reparse_consumers_and_match_full_analysis() -> TestResult {
    let p = Project::new()?;
    let header = "inline int pick(int n) { return n; }\ninline double pick(double n) { return n; }\nusing Value = int;\n";
    p.write("api.hpp", header)?;
    p.write(
        "main.cpp",
        "#include \"api.hpp\"\nint caller() { Value n = 1; return int(pick(n)); }\n",
    )?;
    p.write("other.cpp", "int unrelated() { return 1; }\n")?;
    p.build(false)?;
    p.write("api.hpp", &header.replace("Value = int", "Value = double"))?;
    let update = p.build(false)?;
    assert_eq!(update.parsed_files, 2);
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(relationships(&rows).iter().any(|r| {
        r.rule == "clang_semantic_call"
            && r.target
                .as_ref()
                .is_some_and(|t| t.name == "pick" && t.signature.contains("double"))
    }));
    let incremental = snapshot::normalized_index(&update)?;
    let full = p.build(true)?;
    assert_eq!(incremental, snapshot::normalized_index(&full)?);
    Ok(())
}

#[test]
fn argument_only_edits_change_native_targets_without_rewriting_other_calls() -> TestResult {
    let p = Project::new()?;
    let code = "int pick(int n){return n;} double pick(double n){return n;}\nint caller(){return int(pick(1));}\nint stable(){return pick(7);}\n";
    p.write("main.cpp", code)?;
    p.build(false)?;
    p.write("main.cpp", &code.replace("pick(1)", "pick(2.0)"))?;
    let update = p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(relationships(&rows).iter().any(|r| {
        r.rule == "clang_semantic_call"
            && r.target
                .as_ref()
                .is_some_and(|t| t.signature.contains("double"))
    }));
    assert_eq!(
        snapshot::normalized_index(&update)?,
        snapshot::normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn compile_argument_changes_and_historical_queries_use_the_selected_checkpoint() -> TestResult {
    let p = Project::new()?;
    p.write("main.cpp", "int pick(int n){return n;}double pick(double n){return n;}int caller(){return int(pick(ARG));}")?;
    let config = r#"[{"directory":".","file":"main.cpp","arguments":["clang++","-DARG=1","-c","main.cpp"]}]"#;
    p.write("compile_commands.json", config)?;
    let first = p.build(false)?;
    let revision = first.history.as_ref().ok_or("history")?.checkpoint.clone();
    p.write("compile_commands.json", &config.replace("ARG=1", "ARG=1.5"))?;
    let update = p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(relationships(&rows).iter().any(|r| {
        r.rule == "clang_semantic_call"
            && r.target
                .as_ref()
                .is_some_and(|t| t.signature.contains("double"))
    }));
    assert_eq!(
        snapshot::normalized_index(&update)?,
        snapshot::normalized_index(&p.build(true)?)?
    );
    let mut old = Collected::default();
    p.engine.query(
        &p.options(),
        &Query::Deps {
            selector: selector("main.cpp", "caller"),
        },
        &QueryOptions {
            revision: Some(revision),
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut old,
    )?;
    assert!(relationships(&old).iter().any(|r| {
        r.rule == "clang_semantic_call"
            && r.target
                .as_ref()
                .is_some_and(|t| !t.signature.contains("double"))
    }));
    Ok(())
}

#[test]
fn compiler_plugins_and_response_files_are_diagnosed_without_running_them() -> TestResult {
    for arguments in [
        r#"["clang++","-Xclang","-load","evil.so","main.cpp"]"#,
        r#"["clang++","@outside.rsp","main.cpp"]"#,
    ] {
        let p = Project::new()?;
        p.write(
            "main.cpp",
            "int target(){return 1;}int caller(){return target();}",
        )?;
        p.write(
            "compile_commands.json",
            &format!(r#"[{{"directory":".","file":"main.cpp","arguments":{arguments}}}]"#),
        )?;
        let report = p.build(false)?;
        let connection =
            rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
        let count: usize = connection.query_row(
            "SELECT count(*) FROM diagnostics WHERE code='clang_semantic_incomplete'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);
        let rows = p.query(Query::Deps {
            selector: selector("main.cpp", "caller"),
        })?;
        assert!(
            relationships(&rows)
                .iter()
                .any(|r| r.target.as_ref().is_some_and(|t| t.name == "target"))
        );
    }
    Ok(())
}

#[test]
fn object_macro_function_names_keep_the_native_definition_location() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.c",
        "#define NAME actual\nint NAME(void) { return 1; }\nint caller(void) { return NAME(); }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.c", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.rule == "clang_semantic_call"
                && r.target.as_ref().is_some_and(|t| t.name == "actual"))
    );
    Ok(())
}

#[test]
fn compile_optimization_and_thread_flags_preserve_preprocessor_context() -> TestResult {
    let p = Project::new()?;
    p.write("main.cpp", "int pick(int n){return n;} double pick(double n){return n;}\n#if defined(__OPTIMIZE__) && defined(_REENTRANT)\n#define ARG 1.5\n#else\n#define ARG 1\n#endif\nint caller(){return int(pick(ARG));}")?;
    p.write("compile_commands.json", r#"[{"directory":".","file":"main.cpp","arguments":["clang++","-O2","-pthread","-c","main.cpp"]}]"#)?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.rule == "clang_semantic_call"
                && r.target
                    .as_ref()
                    .is_some_and(|t| t.signature.contains("double"))),
        "{rows:?}"
    );
    Ok(())
}

#[test]
fn a_conditional_first_token_does_not_condition_later_file_definitions() -> TestResult {
    let p = Project::new()?;
    p.write(
        "api.hpp",
        "inline int pick(int n){return n;}\nusing Value = int;",
    )?;
    p.write("main.cpp", "#if __has_include(\"outside.h\")\n#error inaccessible_header\n#endif\n#include <stddef.h>\n#include \"api.hpp\"\n#define DEFINE(name) int name(int value){return pick(value);}\nDEFINE(expanded)\nint caller(){Value value=1;return pick(value)+expanded(2);}")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.cpp".into(),
    })?;
    let callers: Vec<_> = rows
        .rows
        .iter()
        .filter_map(|r| match r {
            QueryRow::Symbol(s) if s.name == "caller" => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(callers.len(), 1, "{callers:?}");
    assert!(callers[0].condition.is_none(), "{callers:?}");
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.rule == "clang_semantic_call"
                && r.target.as_ref().is_some_and(|t| t.name == "expanded"))
    );
    Ok(())
}
