// Purpose: Go 본문 변경의 정의 재사용·interface dispatch·혼합 입력 일관성 검증.
// Pattern: Integration Test.
// Usage: engine 테스트가 부분 갱신과 full 분석을 비교.
// Related: crates/kodstar-core/src/application/analysis/incremental.rs, crates/kodstar-core/src/application/analysis/resolve/go/interfaces.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;
use rusqlite::Connection;

#[test]
fn go_body_edits_keep_unaffected_rows_and_rebuild_call_dispatch() -> TestResult {
    let p = Project::new()?;
    p.write("go.mod", "module example.test/api\ngo 1.23\n")?;
    let source = "package api\ntype Reader interface { Read() }\ntype Worker struct{}\nfunc (w Worker) Read() {}\nfunc caller(r Reader) { r.Read() }\n";
    p.write("api.go", source)?;
    p.write(
        "stable.go",
        "package api\nfunc helper() {}\nfunc stable() { helper() }\n",
    )?;
    p.write("unrelated.js", "function other() {}")?;
    let mut previous = p.build(false)?;
    for body in ["r.Read(); r.Read()", "missing()", "r.Read()"] {
        let database =
            Connection::open(std::path::Path::new(&previous.cache).join("index.sqlite"))?;
        packed_mutations::mark(&database, "stable.go")?;
        postings::register(&database)?;
        let before: i64 = database.query_row("SELECT MIN(r.id) FROM relationships r JOIN facts u ON u.id=r.fact_id JOIN files f ON f.id=u.file_id WHERE f.path=CAST('stable.go' AS BLOB)", [], |r| r.get(0))?;
        drop(database);
        p.write(
            "api.go",
            &source.replace("r.Read() }", &format!("{body} }}")),
        )?;
        let changed = p.build(false)?;
        let database = Connection::open(std::path::Path::new(&changed.cache).join("index.sqlite"))?;
        postings::register(&database)?;
        let after: i64 = database.query_row("SELECT MIN(r.id) FROM relationships r JOIN facts u ON u.id=r.fact_id JOIN files f ON f.id=u.file_id WHERE f.path=CAST('stable.go' AS BLOB)", [], |r| r.get(0))?;
        assert_eq!(
            before, after,
            "unrelated relationships rewritten for {body}"
        );
        let rows = p.query(Query::Explain {
            selector: selector("api.go", "caller"),
        })?;
        let dispatch = relationships(&rows)
            .iter()
            .filter(|r| r.relation == "dispatch_candidate")
            .map(|r| r.count)
            .sum::<u64>();
        assert_eq!(
            dispatch,
            if body == "missing()" {
                0
            } else if body.contains(';') {
                2
            } else {
                1
            }
        );
        let partial = normalized_index(&changed)?;
        let full = p.build(true)?;
        assert_eq!(partial, normalized_index(&full)?);
        previous = full;
    }
    Ok(())
}

#[test]
fn go_definition_changes_revisit_successful_and_failed_lookup_consumers() -> TestResult {
    let p = Project::new()?;
    p.write("go.mod", "module example.test/root\ngo 1.23\n")?;
    p.write("api/defs.go", "package api\nfunc First() {}\n")?;
    p.write("caller.go", "package main\nimport \"example.test/root/api\"\nfunc caller() { api.First(); api.Missing() }\n")?;
    p.write(
        "isolated/stable.go",
        "package isolated\nfunc helper() {}\nfunc stable() { helper() }\n",
    )?;
    let mut previous = p.build(false)?;
    for step in [
        "rename",
        "new_target",
        "duplicate",
        "delete",
        "type_boundary",
    ] {
        let db = Connection::open(std::path::Path::new(&previous.cache).join("index.sqlite"))?;
        packed_mutations::mark(&db, "isolated/stable.go")?;
        postings::register(&db)?;
        let ids = |db: &Connection| -> rusqlite::Result<i64> {
            db.query_row("SELECT MIN(r.id) FROM relationships r JOIN facts u ON u.id=r.fact_id JOIN files f ON f.id=u.file_id WHERE f.path=CAST('isolated/stable.go' AS BLOB)",[],|r|r.get(0))
        };
        let before = ids(&db)?;
        drop(db);
        match step {
            "rename" => p.write("api/defs.go", "package api\nfunc Other() {}\n")?,
            "new_target" => p.write("api/new.go", "package api\nfunc Missing() {}\n")?,
            "duplicate" => p.write("api/duplicate.go", "package api\nfunc Missing() {}\n")?,
            "delete" => std::fs::remove_file(p.root.join("api/new.go"))?,
            _ => p.write("api/defs.go", "package api\ntype Other struct{}\n")?,
        }
        let changed = p.build(false)?;
        let db = Connection::open(std::path::Path::new(&changed.cache).join("index.sqlite"))?;
        postings::register(&db)?;
        if step != "type_boundary" {
            assert_eq!(ids(&db)?, before, "unrelated rows rewritten for {step}");
        }
        let partial = normalized_index(&changed)?;
        let full = p.build(true)?;
        assert_eq!(partial, normalized_index(&full)?, "{step}");
        previous = full;
    }
    Ok(())
}
