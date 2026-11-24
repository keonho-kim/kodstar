// Purpose: 증분 재사용과 제한된 관계 결과 스트리밍의 일관성 검증.
// Pattern: Integration Test.
// Usage: engine 테스트가 독립 프로젝트에서 정규화한 분석 결과와 원자성을 비교.
// Related: crates/kodstar-core/tests/support/snapshot.rs, crates/kodstar-core/src/application/analysis/incremental.rs.

use crate::support::{
    packed_mutations::{mark_relationship_ids, relationship_ids},
    snapshot::normalized_index,
    *,
};
use kodstar_core::*;

#[test]
fn relationship_stream_shares_a_repeated_source_across_distinct_targets() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.rs",
        "fn a() {} fn b() {} fn c() {} fn caller() { a(); b(); c(); }",
    )?;
    p.build(false)?;
    let output = p.query(Query::Deps {
        selector: selector("main.rs", "caller"),
    })?;
    let rows = output
        .rows
        .iter()
        .filter_map(|row| match row {
            QueryRow::Relationship(edge) => Some(edge),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(rows.len() >= 3);
    for row in &rows[1..] {
        assert!(std::sync::Arc::ptr_eq(&rows[0].source, &row.source));
    }
    Ok(())
}

#[test]
fn relationship_batches_preserve_prefixes_counts_and_shared_sources_at_limits() -> TestResult {
    let p = Project::new()?;
    let mut source = String::new();
    for n in 0..273 {
        source.push_str(&format!("fn leaf{n}() {{}}\n"));
    }
    source.push_str("fn caller() {\n");
    for n in 0..273 {
        source.push_str(&format!("leaf{n}(); leaf{n}();\n"));
    }
    source.push_str("}\n");
    p.write("main.rs", &source)?;
    p.build(false)?;
    let query = Query::Deps {
        selector: selector("main.rs", "caller"),
    };
    let full = p.query(query.clone())?;
    let edges: Vec<_> = full
        .rows
        .iter()
        .filter_map(|row| match row {
            QueryRow::Relationship(edge) => Some(edge),
            _ => None,
        })
        .collect();
    assert_eq!(edges.len(), 273);
    for edge in &edges {
        assert_eq!(edge.count, 2);
        assert!(std::sync::Arc::ptr_eq(&edges[0].source, &edge.source));
    }
    for limit in [1, 3, 31, 32, 33, 127, 128, 129, 255, 256, 273, 274] {
        let mut output = Collected::default();
        p.engine.query(
            &p.options(),
            &query,
            &QueryOptions {
                refresh: Refresh::None,
                limit: Limit::Count(std::num::NonZeroUsize::new(limit).unwrap()),
                ..QueryOptions::default()
            },
            &Cancellation::default(),
            &mut output,
        )?;
        let expected = &full.rows[..limit.min(full.rows.len())];
        assert_eq!(
            serde_json::to_value(&output.rows)?,
            serde_json::to_value(expected)?
        );
        assert_eq!(output.summary.as_ref().unwrap().truncated, limit < 273);
    }
    Ok(())
}

#[test]
fn oversized_relationship_projections_fall_back_without_losing_rows() -> TestResult {
    let p = Project::new()?;
    let mut source = String::new();
    let names: Vec<_> = (0..4)
        .map(|n| format!("leaf{n}{}", "x".repeat(40000)))
        .collect();
    for n in 0..128 {
        source.push_str(&format!("fn small{n}() {{}}\n"));
    }
    for name in &names {
        source.push_str(&format!("fn {name}() {{}}\n"));
    }
    source.push_str("fn caller() {\n");
    for n in 0..128 {
        source.push_str(&format!("small{n}();\n"));
    }
    for name in &names {
        source.push_str(&format!("{name}();\n"));
    }
    source.push_str("}\n");
    p.write("main.rs", &source)?;
    p.build(false)?;
    let output = p.query(Query::Deps {
        selector: selector("main.rs", "caller"),
    })?;
    let names: Vec<_> = output
        .rows
        .iter()
        .filter_map(|row| match row {
            QueryRow::Relationship(edge) => edge.target.as_ref().map(|target| target.name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names.len(), 132);
    assert_eq!(names.iter().filter(|name| name.len() > 40000).count(), 4);
    assert!(!output.summary.as_ref().unwrap().truncated);
    Ok(())
}

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
fn cancellation_before_extraction_preserves_the_committed_generation() -> TestResult {
    let p = Project::new()?;
    let source = "fn first() {} fn second() {} fn caller() { first(); }";
    p.write("main.rs", source)?;
    let first = p.build(false)?;
    let expected = normalized_index(&first)?;
    p.write("main.rs", &source.replace("{ first(); }", "{ second(); }"))?;
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
        result.expect_err("cancelled extraction").kind,
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
    Ok(())
}

#[test]
fn cached_header_language_is_reused_and_changed_content_is_reclassified() -> TestResult {
    let p = Project::new()?;
    let license = format!("/*\n{}*/\n", "license text\n".repeat(800));
    p.write("api.h", &format!("{license}class Target {{}};"))?;
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
    p.write("api.h", &format!("{license}struct Target {{}};"))?;
    p.build(false)?;
    assert_eq!(language(&p)?, "c");
    Ok(())
}
