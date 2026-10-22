// Purpose: 삭제된 정의의 이전 사용처와 비교 전체 한도 검증.
// Pattern: Integration Test.
// Usage: engine 회귀 테스트가 두 체크포인트를 비교.
// Related: crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn changes_impact_includes_callers_of_a_deleted_definition() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn target() {}\nfn caller() { target(); }")?;
    p.build(false)?;
    p.write("main.rs", "fn caller() { target(); }")?;
    let mut result = Collected::default();
    p.engine.query(
        &p.options(),
        &Query::Changes { since: "c1".into() },
        &QueryOptions::default(),
        &Cancellation::default(),
        &mut result,
    )?;
    assert!(result.rows.iter().any(|r|matches!(r,QueryRow::History(HistoryRow::Impact{checkpoint,relationship}) if checkpoint=="c1" && relationship.source.name=="caller")));
    let metadata = result.metadata.ok_or("metadata")?;
    assert_eq!(
        metadata
            .history
            .ok_or("history")?
            .comparison
            .ok_or("comparison")?
            .from
            .as_deref(),
        Some("c1")
    );
    Ok(())
}

#[test]
fn unchanged_comparison_returns_no_impact_without_a_second_analysis_cache() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn stable() {}")?;
    let report = p.build(false)?;
    let mut result = Collected::default();
    let summary = p.engine.query(
        &p.options(),
        &Query::Changes { since: "c1".into() },
        &QueryOptions::default(),
        &Cancellation::default(),
        &mut result,
    )?;
    assert_eq!(summary.result_count, 0);
    assert!(summary.complete);
    assert!(
        !std::path::Path::new(&report.cache)
            .join("past.sqlite")
            .exists()
    );
    Ok(())
}
