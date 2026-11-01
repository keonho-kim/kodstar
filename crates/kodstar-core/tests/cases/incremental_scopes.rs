// Purpose: namespace 끝 변경의 관계 보존과 실제 위치·의미 변경의 재해석 검증.
// Pattern: Differential Regression Test.
// Usage: engine 통합 검사가 같은 파일의 증분 결과를 전체 분석과 비교.
// Related: crates/kodstar-core/src/application/analysis/workers.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{snapshot::normalized_index, *};
use rusqlite::Connection;
use std::path::Path;

#[test]
fn scope_end_only_changes_preserve_relationships_and_refresh_all_scope_ends() -> TestResult {
    let p = Project::new()?;
    let source = "<?php\nnamespace Example;\ninterface Contract { public function work(); }\nclass Worker implements Contract { public function work() {} }\nfunction caller(Worker $worker) { $worker->work(); }\n";
    p.write("api.php", source)?;
    let initial = p.build(false)?;
    let c = Connection::open(Path::new(&initial.cache).join("index.sqlite"))?;
    packed_mutations::mark(&c, "api.php")?;
    postings::register(&c)?;
    let ids = |c: &Connection| -> rusqlite::Result<Vec<i64>> {
        c.prepare("SELECT id FROM relationships ORDER BY id")?
            .query_map([], |r| r.get(0))?
            .collect()
    };
    let expected = ids(&c)?;
    assert!(!expected.is_empty());
    drop(c);

    p.write("api.php", &format!("{source}\n"))?;
    let changed = p.build(false)?;
    assert_eq!(changed.history.as_ref().ok_or("history")?.checkpoint, "c2");
    let c = Connection::open(Path::new(&changed.cache).join("index.sqlite"))?;
    postings::register(&c)?;
    assert_eq!(ids(&c)?, expected, "scope-only edit rebuilt relationships");
    drop(c);
    assert_eq!(
        normalized_index(&changed)?,
        normalized_index(&p.build(true)?)?
    );

    for changed_source in [
        source.replace("namespace Example;", "namespace Example;\n\n"),
        source.replace("$worker->work();", "$worker->missing();"),
    ] {
        p.write("api.php", &changed_source)?;
        let incremental = p.build(false)?;
        assert_eq!(
            normalized_index(&incremental)?,
            normalized_index(&p.build(true)?)?
        );
    }
    Ok(())
}
