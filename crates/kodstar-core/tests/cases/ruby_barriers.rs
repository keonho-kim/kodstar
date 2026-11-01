// Purpose: Ruby 소유자·파일·전역 표식 추가·제거가 다음 해석에 반영됨을 검증.
// Pattern: Differential Regression Test.
// Usage: engine 통합 검사가 같은 작업 공간의 반복 갱신을 전체 분석과 비교.
// Related: crates/kodstar-core/src/application/analysis/resolve/ruby/barriers.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;

#[test]
fn barrier_presence_and_absence_are_recomputed_for_each_resolution() -> TestResult {
    let p = Project::new()?;
    let source = "class Worker\n def run; end\n def entry; run; end\nend\n";
    let scoped = source.replace(" def entry;", " remove_method :missing\n def entry;");
    for (source, expected) in [
        (source.to_owned(), "resolved"),
        (scoped, "candidate"),
        (format!("{source}using Unknown\n"), "candidate"),
        (
            format!("{source}Other.class_eval('ignored')\n"),
            "candidate",
        ),
        (source.to_owned(), "resolved"),
    ] {
        p.write("main.rb", &source)?;
        let incremental = p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector("main.rb", "Worker::entry"),
        })?;
        assert!(
            relationships(&rows).iter().any(|r| r.expression == "run"
                && r.resolution == expected
                && r.target
                    .as_ref()
                    .is_some_and(|s| s.qualified_name == "Worker::run")),
            "{:?}",
            rows.rows
        );
        assert_eq!(
            normalized_index(&incremental)?,
            normalized_index(&p.build(true)?)?
        );
    }
    Ok(())
}
