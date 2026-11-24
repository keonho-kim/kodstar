// Purpose: 의미 사실이 작은 대용량 include 입력의 독립 예산 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 검사가 fact batch와 무관하게 header 입력을 처리하는지 확인.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/tests/engine.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn include_bytes_are_separate_from_fact_transport_and_native_output() -> TestResult {
    let p = Project::new()?;
    for i in 0..3 {
        p.write(
            &format!("part{i}.hpp"),
            &format!("/*{}*/\nint part{i}(){{return {i};}}", "x".repeat(1450000)),
        )?;
    }
    p.write("main.cpp","#include \"part0.hpp\"\n#include \"part1.hpp\"\n#include \"part2.hpp\"\nint leaf(int n){return n;}int caller(){return leaf(1);}")?;
    let report = p.build(false)?;
    let c = rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    let (status,detail):(String,Option<String>)=c.query_row("SELECT n.status,n.detail FROM native_analysis n JOIN files_rows f ON f.id=n.file_id WHERE f.path=?1",[b"main.cpp".as_slice()],|r|Ok((r.get(0)?,r.get(1)?)))?;
    assert_eq!(status, "complete", "{detail:?}");
    let rows = p.query(Query::Explain {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.rule == "clang_semantic_call"
                && r.target.as_ref().is_some_and(|t| t.name == "leaf"))
    );
    Ok(())
}
