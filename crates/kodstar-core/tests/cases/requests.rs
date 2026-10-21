// Purpose: 공개 조회 입력의 사전 검증과 모호한 후보 출력 한도 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;
use std::num::NonZeroUsize;

#[test]
fn file_connections_are_sorted_across_both_directions() -> TestResult {
    let project = Project::new()?;
    project.write(
        "main.ts",
        "import { target } from './z'; export function caller() { target(); }",
    )?;
    project.write(
        "a.ts",
        "import { caller } from './main'; function incoming() { caller(); }",
    )?;
    project.write("z.ts", "export function target() {}")?;
    project.build(false)?;
    let mut sink = Collected::default();
    project.engine.query(
        &project.options(),
        &Query::Files {
            file: "main.ts".into(),
        },
        &QueryOptions {
            refresh: Refresh::None,
            relations: vec![RelationKind::Calls],
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut sink,
    )?;
    let files: Vec<_> = sink
        .rows
        .iter()
        .filter_map(|row| match row {
            QueryRow::File(row) => Some(row.file.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(files, ["a.ts", "z.ts"]);
    Ok(())
}

#[test]
fn invalid_engine_requests_are_rejected_before_cache_access() -> TestResult {
    let project = Project::new()?;
    let mut invalid_position = selector("main.rs", "caller");
    invalid_position.at = Some((0, 1));
    for (query, options) in [
        (Query::Symbols { file: "".into() }, QueryOptions::default()),
        (
            Query::Search {
                name: "".into(),
                prefix: false,
            },
            QueryOptions::default(),
        ),
        (
            Query::Related {
                selector: invalid_position,
            },
            QueryOptions::default(),
        ),
        (
            Query::Impact {
                file: "main.rs".into(),
                name: None,
                at: Some((1, 1)),
            },
            QueryOptions::default(),
        ),
        (
            Query::Symbols {
                file: "main.rs".into(),
            },
            QueryOptions {
                depth: Depth::Transitive,
                ..QueryOptions::default()
            },
        ),
        (
            Query::Impact {
                file: "main.rs".into(),
                name: None,
                at: None,
            },
            QueryOptions {
                direction: Some(Direction::Out),
                ..QueryOptions::default()
            },
        ),
    ] {
        let error = project
            .engine
            .query(
                &project.options(),
                &query,
                &options,
                &Cancellation::default(),
                &mut Collected::default(),
            )
            .expect_err("invalid public request");
        assert_eq!(error.kind, ErrorKind::Argument);
    }
    assert!(!project.directory.join("cache").exists());
    Ok(())
}

#[test]
fn ambiguous_selection_respects_the_result_limit() -> TestResult {
    let project = Project::new()?;
    project.write("main.py", "class A:\n    def target(self): pass\nclass B:\n    def target(self): pass\nclass C:\n    def target(self): pass\n")?;
    project.build(false)?;
    let mut sink = Collected::default();
    let error = project
        .engine
        .query(
            &project.options(),
            &Query::Related {
                selector: selector("main.py", "target"),
            },
            &QueryOptions {
                refresh: Refresh::None,
                limit: Limit::Count(NonZeroUsize::new(1).ok_or("limit")?),
                ..QueryOptions::default()
            },
            &Cancellation::default(),
            &mut sink,
        )
        .expect_err("selection is ambiguous");
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert_eq!(sink.rows.len(), 1);
    assert!(sink.summary.ok_or("summary")?.truncated);
    Ok(())
}
