// Purpose: 관련 없는 파일 수와 독립적인 모듈 후보 조회의 작업량 검증.
// Pattern: Query Regression Test.
// Usage: module_files를 실제 SQLite index와 progress 예산으로 실행.
// Related: crates/kodstar-core/src/application/analysis/resolve/modules/files.rs, crates/kodstar-core/src/infrastructure/index/mod.rs.

use super::*;
use crate::application::analysis::resolve::{prepared::Prepared, publication};
use crate::domain::resolve::namespaces::Namespace;
use crate::infrastructure::index::{self, FileRecord, cached::Sql};

#[test]
fn exact_module_lookup_does_not_scan_unrelated_files()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let c = index::open(
        &temporary.path().join("index.sqlite"),
        temporary.path(),
        true,
        1024 * 1024,
    )?;
    let sql = Sql::new(&c)?;
    c.execute_batch("BEGIN")?;
    for i in 0..1000 {
        FileRecord {
            path: format!("module{i}.rs").into(),
            language: "rust".into(),
            module: format!("crate::module{i}"),
            package: "fixture".into(),
            content_hash: [0; 32],
            size: 0,
            status: "analyzed",
        }
        .persist(&sql)?;
    }
    c.execute_batch("COMMIT")?;
    c.execute_batch(include_str!("../../staging.sql"))?;
    publication::reset(&sql)?;
    let prepared = Prepared::new(&c)?;
    let context = FileContext {
        id: 1,
        language: Language::Rust,
        input_format: Some(Language::Rust),
        module: "crate".into(),
        package: "fixture".into(),
        path: "lib.rs".into(),
        declared_namespace: String::new(),
    };
    let mut lookup = Lookup {
        connection: &sql,
        fact: None,
        source: None,
        relation: "references",
        external_scope_id: 0,
        scope_sequence: 0,
        external_present: false,
        reason: None,
        rule: "lexical_binding",
        source_override: None,
        member_depth: 0,
        candidate_only: false,
        language: Language::Rust,
        namespace: Namespace::Any,
        receiver: false,
        ruby_side_checked: false,
        ruby_side: None,
        prepared: &prepared,
    };
    let mut calls = 0;
    c.progress_handler(
        1000,
        Some(move || {
            calls += 1;
            calls > 20
        }),
    )?;
    lookup.module_files(&context, 0, "crate::module999", 0)?;
    c.progress_handler(0, None::<fn() -> bool>)?;
    let modules: usize =
        c.query_row("SELECT count(*) FROM resolution_modules", [], |r| r.get(0))?;
    assert_eq!(modules, 1);
    let selected: String = c.query_row(
        "SELECT f.module FROM files f JOIN resolution_modules m ON m.id=f.id",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(selected, "crate::module999");
    drop(prepared);
    drop(sql);
    drop(c);
    temporary.close()?;
    Ok(())
}
