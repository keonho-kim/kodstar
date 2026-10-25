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

#[test]
fn changed_import_targets_preserve_unrelated_facts_in_the_same_consumer_file() -> TestResult {
    let p = Project::new()?;
    p.write("go.mod", "module example.test/root\ngo 1.23\n")?;
    p.write("api/defs.go", "package api\nfunc First() {}\n")?;
    p.write("caller.go", "package main\nimport \"example.test/root/api\"\nfunc local() {}\nfunc caller() { local(); api.First(); api.Missing() }\n")?;
    let initial = p.build(false)?;
    let db = Connection::open(std::path::Path::new(&initial.cache).join("index.sqlite"))?;
    packed_mutations::mark(&db, "caller.go")?;
    postings::register(&db)?;
    let stable = |db: &Connection| -> rusqlite::Result<i64> {
        db.query_row(
            "SELECT r.id FROM relationships r JOIN facts f ON f.id=r.fact_id
            JOIN files source ON source.id=f.file_id WHERE source.path=CAST('caller.go' AS BLOB)
            AND f.relation='calls' AND f.expression='local'",
            [],
            |r| r.get(0),
        )
    };
    let before = stable(&db)?;
    drop(db);
    p.write("api/new.go", "package api\nfunc Missing() {}\n")?;
    let changed = p.build(false)?;
    let db = Connection::open(std::path::Path::new(&changed.cache).join("index.sqlite"))?;
    postings::register(&db)?;
    assert_eq!(stable(&db)?, before);
    drop(db);
    let partial = normalized_index(&changed)?;
    assert_eq!(partial, normalized_index(&p.build(true)?)?);
    std::fs::remove_file(p.root.join("api/new.go"))?;
    let changed = p.build(false)?;
    assert_eq!(
        normalized_index(&changed)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn empty_provider_packages_are_invalidated_through_modules_workspaces_and_replacements()
-> TestResult {
    for mapping in ["module", "workspace", "replacement"] {
        let p = Project::new()?;
        let (caller, definition) = match mapping {
            "workspace" => {
                p.write("go.work", "go 1.23\nuse (\n./app\n./lib\n)\n")?;
                p.write("app/go.mod", "module example.test/app\ngo 1.23\n")?;
                p.write("lib/go.mod", "module example.test/lib\ngo 1.23\n")?;
                ("app/caller.go", "lib/api/defs.go")
            }
            "replacement" => {
                p.write("go.mod", "module example.test/app\ngo 1.23\nrequire example.test/lib v1.0.0\nreplace example.test/lib => ./local\n")?;
                p.write("local/go.mod", "module example.test/lib\ngo 1.23\n")?;
                ("caller.go", "local/api/defs.go")
            }
            _ => {
                p.write("go.mod", "module example.test/lib\ngo 1.23\n")?;
                ("caller.go", "api/defs.go")
            }
        };
        p.write(
            caller,
            "package main\nimport \"example.test/lib/api\"\nfunc caller() { api.Future() }\n",
        )?;
        let initial = p.build(false)?;
        let db = Connection::open(std::path::Path::new(&initial.cache).join("index.sqlite"))?;
        postings::register(&db)?;
        let missing: usize = db.query_row(
            "SELECT count(*) FROM lookup_dependencies WHERE kind='go_package' AND found=0",
            [],
            |r| r.get(0),
        )?;
        assert!(missing > 0, "missing provider was not recorded: {mapping}");
        drop(db);
        p.write(definition, "package api\nfunc Future() {}\n")?;
        let changed = p.build(false)?;
        assert_eq!(
            normalized_index(&changed)?,
            normalized_index(&p.build(true)?)?,
            "{mapping}: added"
        );
        let rows = p.query(Query::Deps {
            selector: selector(caller, "caller"),
        })?;
        assert!(
            relationships(&rows)
                .iter()
                .any(|r| r.target.as_ref().is_some_and(|s| s.name == "Future")),
            "{mapping}: unresolved new target"
        );
        std::fs::remove_file(p.root.join(definition))?;
        let changed = p.build(false)?;
        assert_eq!(
            normalized_index(&changed)?,
            normalized_index(&p.build(true)?)?,
            "{mapping}: deleted"
        );
    }
    Ok(())
}

#[test]
fn large_changed_files_fall_back_without_losing_or_duplicating_facts() -> TestResult {
    let p = Project::new()?;
    p.write("go.mod", "module example.test/api\ngo 1.23\n")?;
    let source = format!(
        "package api\nfunc Alpha() {{}}\nfunc Beta() {{}}\nfunc caller() {{ {} }}\n",
        "Alpha();".repeat(300)
    );
    p.write("api.go", &source)?;
    p.build(false)?;
    p.write("api.go", &source.replace("Alpha();", "Beta();"))?;
    let changed = p.build(false)?;
    assert_eq!(
        normalized_index(&changed)?,
        normalized_index(&p.build(true)?)?
    );
    let rows = p.query(Query::Deps {
        selector: selector("api.go", "caller"),
    })?;
    let count: u64 = relationships(&rows)
        .iter()
        .filter(|r| r.target.as_ref().is_some_and(|s| s.name == "Beta"))
        .map(|r| r.count)
        .sum();
    assert_eq!(count, 300);
    Ok(())
}
