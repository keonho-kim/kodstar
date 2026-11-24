// Purpose: 내장 표준 헤더·quoted compile command·전송 배치 독립성 검증.
// Pattern: Integration Test.
// Usage: engine 통합 검사가 실제 native 호출 근거와 상세 실패를 확인.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/tests/engine.rs.

use crate::support::*;
use kodstar_core::*;

fn native_call(p: &Project, file: &str) -> TestResult {
    let rows = p.query(Query::Explain {
        selector: selector(file, "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|r| r.relation == "calls"
            && r.rule == "clang_semantic_call"
            && r.target.as_ref().is_some_and(|t| t.name == "leaf")),
        "{rows:?}"
    );
    Ok(())
}

#[test]
#[cfg(target_os = "macos")]
fn standard_c_and_cpp_headers_are_available_without_host_sdk_reads() -> TestResult {
    let p = Project::new()?;
    p.write("main.c","#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\nint leaf(int n){return n;}int caller(void){return leaf(1);}\n")?;
    p.write("main.cpp","#include <vector>\n#include <algorithm>\n#include <string>\nint leaf(int n){return n;}double leaf(double n){return n;}int caller(){std::vector<int> values{1};return leaf(values.front());}\n")?;
    let report = p.build(false)?;
    let c = rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    let status: Vec<(String, Option<String>)> = c
        .prepare("SELECT status,detail FROM native_analysis")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    assert!(status.iter().all(|(s, _)| s == "complete"), "{status:?}");
    native_call(&p, "main.c")?;
    native_call(&p, "main.cpp")?;
    Ok(())
}

#[test]
fn quoted_compile_commands_preserve_macro_and_include_context() -> TestResult {
    let p = Project::new()?;
    p.write("headers dir/api.h", "int leaf(int n){return n;}")?;
    p.write("main.cpp","#include <api.h>\n#if SELECTED\nint caller(){return leaf(1);}\n#else\n#error incorrect compile context\n#endif\n")?;
    p.write("compile_commands.json",r#"[{"directory":".","file":"main.cpp","command":"clang++ -DSELECTED=1 -I'headers dir' -c main.cpp -o main.o"}]"#)?;
    p.build(false)?;
    native_call(&p, "main.cpp")?;
    Ok(())
}

#[test]
fn native_work_does_not_depend_on_fact_transport_batch_size() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        &format!(
            "int values[]={{ {} }};int leaf(int n){{return n;}}int caller(){{return leaf(1);}}",
            "1,".repeat(70000)
        ),
    )?;
    for size in [1048576, 4194304] {
        let config = p.directory.join("analysis.toml");
        std::fs::write(&config, format!("fact_batch_bytes={size}\n"))?;
        let mut options = p.options();
        options.config = Some(config);
        let report = p.engine.init(
            &options,
            InitOptions {
                full: true,
                ..InitOptions::default()
            },
            &Cancellation::default(),
            &mut |_| Ok(()),
        )?;
        let c =
            rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
        let (status, detail): (String, Option<String>) =
            c.query_row("SELECT status,detail FROM native_analysis", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        assert_eq!(status, "complete", "batch {size}: {detail:?}");
        native_call(&p, "main.cpp")?;
    }
    Ok(())
}

#[test]
fn missing_headers_have_specific_diagnostics_without_source_excerpts() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        "#include \"not-installed.h\"\nint caller(){return 1;}\n",
    )?;
    let report = p.build(false)?;
    let c = rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    let detail: String = c.query_row("SELECT detail FROM native_analysis", [], |r| r.get(0))?;
    let detail: serde_json::Value = serde_json::from_str(&detail)?;
    assert_eq!(detail["kind"], "MissingHeader");
    assert_eq!(detail["subject"], "not-installed.h");
    Ok(())
}
