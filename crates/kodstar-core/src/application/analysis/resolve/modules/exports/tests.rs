// Purpose: C++ 모듈 이름 후보 조회가 무관한 선언을 순회하지 않는지 검증.
// Pattern: Query Regression Test.
// Usage: 실제 저장 schema·후보 SQL에 SQLite progress 예산을 적용.
// Related: crates/kodstar-core/src/application/analysis/resolve/modules/exports.rs, crates/kodstar-core/src/application/analysis/resolve/modules/candidates.sql.

use super::candidate_name_guard;
use crate::infrastructure::index;
use crate::{Cancellation, Engine, InitOptions, Language, ProjectOptions};
use rusqlite::params;

#[test]
fn cpp_module_candidates_use_the_name_index_for_hits_and_misses()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("project");
    std::fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    let mut source = (0..1000)
        .map(|id| format!("void unrelated{id}() {{}}\n"))
        .collect::<String>();
    source.push_str("void target() {}\n");
    std::fs::write(root.join("main.cpp"), source)?;
    let report = Engine::new(temporary.path().join("cache"))?.init(
        &ProjectOptions::new(root.clone()),
        InitOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    let c = index::open(
        &std::path::Path::new(&report.cache).join("index.sqlite"),
        &root,
        false,
        1024 * 1024,
    )?;
    c.execute_batch(include_str!("../../staging.sql"))?;
    let file: i64 = c.query_row("SELECT id FROM files LIMIT 1", [], |row| row.get(0))?;
    c.execute("INSERT INTO resolution_modules VALUES(7,?1)", [file])?;
    let query = include_str!("../candidates.sql").replace(
        "/* indexed name guard */",
        candidate_name_guard(Language::Cpp),
    );
    let mut calls = 0;
    c.progress_handler(
        100,
        Some(move || {
            calls += 1;
            calls > 20
        }),
    )?;
    for (name, expected) in [("target", 1), ("missing", 0)] {
        c.execute("DELETE FROM resolution_candidates", [])?;
        c.execute(&query, params![name, 7, None::<u32>, name, file, "any"])?;
        assert_eq!(
            c.query_row("SELECT count(*) FROM resolution_candidates", [], |row| row
                .get::<_, u32>(
                0
            ))?,
            expected
        );
    }
    c.progress_handler(0, None::<fn() -> bool>)?;
    drop(c);
    temporary.close()?;
    Ok(())
}
