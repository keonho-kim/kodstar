// Purpose: 사실 재사용·본문 재해석·취소·언어 재판정의 일관성 검증.
// Pattern: Integration Test.
// Usage: engine 테스트가 독립 프로젝트에서 full 결과와 일곱 계약 비교.
// Related: crates/kodstar-core/tests/support/snapshot.rs, crates/kodstar-core/src/application/analysis/incremental.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;

#[test]
fn comments_body_edits_and_new_candidates_match_full_results() -> TestResult {
    let p = Project::new()?;
    let initial = "pub fn first() {}\npub fn second() {}\npub fn caller() { first(); }\n";
    p.write("a.rs", initial)?;
    p.write("b.rs", "pub fn target() {} pub fn stable() { target(); }")?;
    p.write("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'\n")?;
    let mut previous = p.build(false)?;
    let sources = [
        (format!("{initial}// comment\n"), true),
        (initial.replace("{ first(); }", "{ second(); }"), true),
        (
            initial.replace("{ first(); }", "{ first(); second(); }"),
            true,
        ),
        (initial.replace("{ first(); }", "{ missing(); }"), true),
        (
            format!(
                "{}\npub fn missing() {{}}\n",
                initial.replace("{ first(); }", "{ missing(); }")
            ),
            false,
        ),
        (initial.replace("{ first(); }", "{ missing(); }"), false),
    ];
    for (source, preserves_other_facts) in sources {
        let stable = mark_relationship_ids(&previous, "b.rs")?;
        p.write("a.rs", &source)?;
        let updated = p.build(false)?;
        assert_eq!(updated.parsed_files, 1);
        if preserves_other_facts {
            assert_eq!(relationship_ids(&updated, "b.rs")?, stable);
        }
        let incremental = normalized_index(&updated)?;
        let full = p.build(true)?;
        assert_eq!(incremental, normalized_index(&full)?);
        previous = full;
    }
    Ok(())
}

#[test]
fn unchanged_facts_preserve_rows_for_non_rust_and_refresh_root_ranges() -> TestResult {
    let p = Project::new()?;
    let source = "def target(): pass\ndef caller(): target()\n";
    p.write("main.py", source)?;
    let first = p.build(false)?;
    let before = mark_relationship_ids(&first, "main.py")?;
    p.write("main.py", &format!("{source}# 한글 주석\r\n"))?;
    let updated = p.build(false)?;
    assert_eq!(relationship_ids(&updated, "main.py")?, before);
    assert_eq!(updated.generation, first.generation + 1);
    assert_eq!(updated.parsed_files, 1);
    let incremental = normalized_index(&updated)?;
    assert_eq!(incremental, normalized_index(&p.build(true)?)?);
    Ok(())
}

#[test]
fn cancellation_after_reused_or_replayed_file_rolls_back_its_generation() -> TestResult {
    let p = Project::new()?;
    let source = "fn first() {} fn second() {} fn caller() { first(); }";
    p.write("main.rs", source)?;
    let first = p.build(false)?;
    let expected = normalized_index(&first)?;
    for changed in [
        format!("{source}\n// comment\n"),
        source.replace("{ first(); }", "{ second(); }"),
    ] {
        p.write("main.rs", &changed)?;
        let cancel = Cancellation::default();
        let result = p.engine.init(
            &p.options(),
            InitOptions::default(),
            &cancel,
            &mut |event| {
                if event.phase == "parse" {
                    cancel.cancel();
                }
                Ok(())
            },
        );
        assert_eq!(
            result.expect_err("cancelled publication").kind,
            ErrorKind::Cancelled
        );
        assert_eq!(expected, normalized_index(&first)?);
        let metadata = p
            .query(Query::Symbols {
                file: "main.rs".into(),
            })?
            .metadata
            .ok_or("metadata")?;
        assert_eq!(metadata.generation, Some(first.generation));
    }
    Ok(())
}

#[test]
fn cached_header_language_is_reused_and_changed_content_is_reclassified() -> TestResult {
    let p = Project::new()?;
    p.write("api.h", "class Target {};")?;
    p.build(false)?;
    let unchanged = p.engine.init(
        &p.options(),
        InitOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    assert_eq!(unchanged.parsed_files, 0);
    let language = |p: &Project| -> std::result::Result<String, Box<dyn std::error::Error>> {
        let result = p.query(Query::Symbols {
            file: "api.h".into(),
        })?;
        match result.rows.first() {
            Some(QueryRow::Symbol(symbol)) => Ok(symbol.language.clone()),
            _ => Err("header definition missing".into()),
        }
    };
    assert_eq!(language(&p)?, "cpp");
    p.write("api.h", "struct Target {};")?;
    p.build(false)?;
    assert_eq!(language(&p)?, "c");
    Ok(())
}

fn relationship_ids(
    report: &InitReport,
    file: &str,
) -> std::result::Result<Vec<i64>, Box<dyn std::error::Error>> {
    let c = rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    postings::register(&c)?;
    let mut statement = c.prepare("SELECT r.id FROM relationships r JOIN facts u ON u.id=r.fact_id JOIN files f ON f.id=u.file_id WHERE f.display_path=?1 ORDER BY r.id")?;
    Ok(statement
        .query_map([file], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?)
}

fn mark_relationship_ids(
    report: &InitReport,
    file: &str,
) -> std::result::Result<Vec<i64>, Box<dyn std::error::Error>> {
    let c = rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    packed_mutations::mark(&c, file)?;
    drop(c);
    let ids = relationship_ids(report, file)?;
    assert!(!ids.is_empty());
    Ok(ids)
}
