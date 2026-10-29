// Purpose: 제한된 SQL 작업 안의 정확·접두어 검색과 언어별 이름 규칙 검증.
// Pattern: Query Regression Test.
// Usage: search 단위 검사가 실제 엔진 인덱스와 SQLite progress 예산을 사용.
// Related: crates/kodstar-core/src/application/query/search.rs, crates/kodstar-core/src/infrastructure/index/mod.rs.

use super::*;
use crate::{Engine, InitOptions, ProjectOptions, QueryMetadata, QueryRow, QuerySummary};

#[derive(Default)]
struct Names(Vec<String>);
impl QuerySink for Names {
    fn begin(&mut self, _: &QueryMetadata) -> Result<()> {
        Ok(())
    }
    fn row(&mut self, row: &QueryRow) -> Result<()> {
        if let QueryRow::Symbol(symbol) = row {
            self.0.push(symbol.name.clone());
        }
        Ok(())
    }
    fn finish(&mut self, _: &QuerySummary) -> Result<()> {
        Ok(())
    }
}

#[test]
fn selective_search_does_not_scan_unrelated_symbols()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("project");
    std::fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    let source = (0..1000)
        .map(|i| format!("fn target{i:04}() {{}}\n"))
        .collect::<String>();
    std::fs::write(root.join("api.rs"), source)?;
    let engine = Engine::new(temporary.path().join("cache"))?;
    let report = engine.init(
        &ProjectOptions::new(root.clone()),
        InitOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    let c = crate::infrastructure::index::open(
        &std::path::Path::new(&report.cache).join("index.sqlite"),
        &root,
        false,
        1024 * 1024,
    )?;
    for (name, prefix, expected) in [
        ("absent", false, 0),
        ("target0999", false, 1),
        ("target099", true, 10),
    ] {
        let mut calls = 0;
        c.progress_handler(
            1000,
            Some(move || {
                calls += 1;
                calls > 20
            }),
        )?;
        let mut names = Names::default();
        run(
            &c,
            name,
            prefix,
            None,
            &Cancellation::default(),
            &mut names,
            &mut QuerySummary::default(),
        )?;
        assert_eq!(names.0.len(), expected);
        c.progress_handler(0, None::<fn() -> bool>)?;
    }
    drop(c);
    temporary.close()?;
    Ok(())
}
