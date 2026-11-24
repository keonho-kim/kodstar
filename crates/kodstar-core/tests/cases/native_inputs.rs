// Purpose: native 입력 관측 갱신과 의미 사실 재사용의 독립성 검증.
// Pattern: Integration Test.
// Usage: engine 통합 검사가 include 존재 변화·공백 편집을 full 결과와 대조.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/tests/engine.rs.

use crate::support::*;
use kodstar_core::InitReport;

fn environment(report: &InitReport, path: &str) -> TestResultValue {
    let c = rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    postings::register(&c)?;
    Ok(c.query_row(
        "SELECT hex(environment_hash) FROM files WHERE path=?1",
        [path.as_bytes()],
        |r| r.get(0),
    )?)
}
type TestResultValue = std::result::Result<String, Box<dyn std::error::Error>>;

#[test]
fn include_bytes_refresh_without_changing_the_declaration_environment() -> TestResult {
    let p = Project::new()?;
    p.write("part.cpp", "int leaf(int n){return n;}\n")?;
    p.write(
        "main.cpp",
        "#include \"part.cpp\"\nint caller(){return leaf(1);}\n",
    )?;
    p.write(
        "other.cpp",
        "int helper(){return 1;}int other(){return helper();}\n",
    )?;
    let initial = p.build(false)?;
    let previous = environment(&initial, "main.cpp")?;
    p.write("part.cpp", "int leaf(int n){return n;}\n\n")?;
    let updated = p.build(false)?;
    assert_eq!(
        previous,
        environment(&updated, "main.cpp")?,
        "input bytes are not a declaration change"
    );
    assert_eq!(
        snapshot::normalized_index(&updated)?,
        snapshot::normalized_index(&p.build(true)?)?
    );
    let unchanged = p.build(false)?;
    assert_eq!(
        unchanged.parsed_files, 0,
        "refreshed observations must be reusable"
    );
    Ok(())
}

#[test]
fn failed_directory_probes_are_invalidated_by_creation_and_removal() -> TestResult {
    let p = Project::new()?;
    p.write("main.cpp","#if __has_include(\"added/api.h\")\n#include \"added/api.h\"\n#endif\nint local(){return 1;}int caller(){return local();}\n")?;
    p.build(false)?;
    p.write("added/api.h", "int added(){return 2;}\n")?;
    let updated = p.build(false)?;
    assert_eq!(
        snapshot::normalized_index(&updated)?,
        snapshot::normalized_index(&p.build(true)?)?
    );
    std::fs::remove_dir_all(p.root.join("added"))?;
    let updated = p.build(false)?;
    assert_eq!(
        snapshot::normalized_index(&updated)?,
        snapshot::normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn syntax_reparsing_retains_native_observations_until_the_file_finishes() -> TestResult {
    for source in [
        "#include \"api.h\"\n#define DEFINE(name) int name(int n){return leaf(n);}\nDEFINE(expanded)\nint caller(){return expanded(1);}\n",
        "#include \"api.h\"\nint caller(){return\n#if FLAG\nleaf(1)\n#else\nleaf(2)\n#endif\n;}\n",
    ] {
        let p = Project::new()?;
        p.write("api.h", "inline int leaf(int n){return n;}")?;
        p.write("main.cpp", source)?;
        let initial = p.build(false)?;
        let c =
            rusqlite::Connection::open(std::path::Path::new(&initial.cache).join("index.sqlite"))?;
        let observed:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM native_inputs n JOIN files_rows f ON f.id=n.file_id WHERE f.path=?1 AND n.path='api.h')",[b"main.cpp".as_slice()],|r|r.get(0))?;
        assert!(
            observed,
            "syntax recovery must retain include dependencies: {source}"
        );
        p.write("main.cpp", &format!("{source}\n"))?;
        let updated = p.build(false)?;
        assert_eq!(
            snapshot::normalized_index(&updated)?,
            snapshot::normalized_index(&p.build(true)?)?
        );
        p.write("api.h", "inline int leaf(int n){return n+1;}")?;
        let updated = p.build(false)?;
        assert_eq!(
            updated.parsed_files, 2,
            "header and its consumer must be reparsed"
        );
        assert_eq!(
            snapshot::normalized_index(&updated)?,
            snapshot::normalized_index(&p.build(true)?)?
        );
    }
    Ok(())
}
