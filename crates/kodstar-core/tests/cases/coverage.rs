// Purpose: 저장된 세대 통계와 실제 행·rollback의 일치 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트가 독립 임시 프로젝트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/index/statistics.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn generation_coverage_matches_rows_after_update_delete_and_failed_extraction() -> TestResult {
    let p = Project::new()?;
    p.write(
        "lib.rs",
        "fn target() {} fn caller() { target(); absent(); }",
    )?;
    p.write("notes.unknown", "unclassified")?;
    let first = p.build(false)?;
    assert_rows(&first)?;
    let unchanged = p.build(false)?;
    assert!(!unchanged.changed);
    assert_eq!(first.generation, unchanged.generation);
    assert_rows(&unchanged)?;
    p.write("next.rs", "fn next() {}")?;
    let added = p.build(false)?;
    assert_rows(&added)?;
    std::fs::remove_file(p.root.join("lib.rs"))?;
    let removed = p.build(false)?;
    assert_rows(&removed)?;
    p.write("next.rs", "fn broken( {")?;
    assert!(p.build(false).is_err());
    let metadata = p
        .query(Query::Symbols {
            file: "next.rs".into(),
        })?
        .metadata
        .ok_or("query metadata")?;
    assert_eq!(metadata.generation, Some(removed.generation));
    let mut expected = removed.coverage.clone();
    expected.reused_files = 0;
    assert_eq!(
        serde_json::to_value(metadata.coverage)?,
        serde_json::to_value(expected)?
    );
    assert_rows(&removed)?;
    Ok(())
}

fn assert_rows(report: &InitReport) -> TestResult {
    let c = rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    postings::register(&c)?;
    let actual: (u64,u64,u64,u64,u64,u64,u64,u64) = c.query_row(
        "SELECT COUNT(*),COALESCE(SUM(status='analyzed'),0),
          COALESCE(SUM(status='skipped'),0),COALESCE(SUM(status='unsupported'),0),
          COALESCE(SUM(CASE WHEN status='analyzed' THEN size ELSE 0 END),0),
          (SELECT COUNT(DISTINCT logical_key) FROM symbols),
          (SELECT COUNT(*) FROM relationships WHERE resolution='resolved'),
          (SELECT COUNT(DISTINCT fact_id) FROM relationships WHERE resolution IN ('unresolved','ambiguous'))
         FROM files", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)))?;
    let expected = &report.coverage;
    assert_eq!(
        actual,
        (
            expected.scanned_files,
            expected.analyzed_files,
            expected.skipped_files,
            expected.unsupported_files,
            expected.source_bytes,
            expected.symbols,
            expected.relationships,
            expected.unresolved
        )
    );
    let generation: i64 = c.query_row(
        "SELECT generation FROM coverage WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(generation, report.generation);
    Ok(())
}
