// Purpose: 큰 정상 그래프에서 작은 본문 변경의 통계와 전체 분석 일치 검증.
// Pattern: Integration Test.
// Usage: engine 테스트가 해결·미해결·복수 사용의 통계 차이를 검증.
// Related: crates/kodstar-core/src/infrastructure/index/statistics/delta.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{snapshot::normalized_index, *};

#[test]
fn small_body_changes_publish_the_same_coverage_as_full_analysis() -> TestResult {
    let p = Project::new()?;
    p.write("go.mod", "module example.test/api\ngo 1.23\n")?;
    p.write(
        "stable.go",
        &format!(
            "package api\nfunc stable() {{ {} }}\n",
            "Alpha();".repeat(700)
        ),
    )?;
    let source = "package api\nfunc Alpha() {}\nfunc Beta() {}\nfunc caller() { Alpha() }\n";
    p.write("api.go", source)?;
    p.build(false)?;
    for body in ["Alpha(); missing()", "Beta(); Beta()", "Alpha()"] {
        p.write(
            "api.go",
            &source.replace("Alpha() }", &format!("{body} }}")),
        )?;
        let partial = p.build(false)?;
        let snapshot = normalized_index(&partial)?;
        let full = p.build(true)?;
        let a = partial.coverage;
        let b = full.coverage.clone();
        assert_eq!(
            (a.symbols, a.relationships, a.unresolved),
            (b.symbols, b.relationships, b.unresolved),
            "{body}"
        );
        assert_eq!(snapshot, normalized_index(&full)?, "{body}");
    }
    Ok(())
}

#[test]
fn declaration_additions_do_not_count_retained_import_relationships_twice() -> TestResult {
    let p = Project::new()?;
    p.write("api.py", "def original(): pass\n")?;
    p.write(
        "stable.py",
        &format!(
            "from api import *\ndef stable():\n{}",
            "    original()\n".repeat(700)
        ),
    )?;
    let initial = p.build(false)?;
    assert!(initial.coverage.relationships >= 512);
    p.write("api.py", "def original(): pass\ndef kodstar_added_leaf(): pass\ndef kodstar_added_caller(): kodstar_added_leaf()\n")?;
    let incremental = p.build(false)?;
    let snapshot = normalized_index(&incremental)?;
    let full = p.build(true)?;
    assert_eq!(snapshot, normalized_index(&full)?);
    assert_eq!(
        incremental.coverage.relationships,
        full.coverage.relationships
    );
    assert_eq!(incremental.coverage.unresolved, full.coverage.unresolved);
    Ok(())
}
