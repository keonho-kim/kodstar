// Purpose: C++ namespace의 성공·실패 조회가 무관한 심볼 수를 순회하지 않는지 검증.
// Pattern: Query Regression Test.
// Usage: 실제 엔진 인덱스에 SQLite progress 예산을 적용해 namespace adapter를 검사.
// Related: crates/kodstar-core/src/application/analysis/resolve/namespaces/cxx.rs, crates/kodstar-core/src/infrastructure/index/mod.rs.

use super::cxx;
use crate::infrastructure::index::{self, cached::Sql};
use crate::{Cancellation, Engine, InitOptions, ProjectOptions};

#[test]
fn cpp_namespace_queries_do_not_scan_unrelated_symbols() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("project");
    std::fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    let mut source = (0..1000)
        .map(|id| format!("struct Unrelated{id} {{}};\n"))
        .collect::<String>();
    source.push_str("namespace App { namespace Target {} }\n");
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
    c.execute_batch(include_str!("../staging.sql"))?;
    let package: String = c.query_row("SELECT package FROM files LIMIT 1", [], |row| row.get(0))?;
    let mut calls = 0;
    c.progress_handler(
        100,
        Some(move || {
            calls += 1;
            calls > 20
        }),
    )?;
    let sql = Sql::new(&c)?;
    assert!(cxx::declared(&sql, &package, "App::Target")?);
    assert!(!cxx::declared(&sql, &package, "App::Missing")?);
    assert!(!cxx::declared(&sql, "other package", "App::Target")?);
    cxx::owners(&sql, &package, "App::Target", 7)?;
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM resolution_module_owners WHERE depth=7",
            [],
            |row| row.get::<_, u32>(0)
        )?,
        1
    );
    c.progress_handler(0, None::<fn() -> bool>)?;
    drop(sql);
    drop(c);
    temporary.close()?;
    Ok(())
}
