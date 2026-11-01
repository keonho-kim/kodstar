// Purpose: 기본 엔진 조회·갱신·실패·경로 계약 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트 진입점이 사례 모듈로 연결.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;
use std::path::{Path, PathBuf};

#[test]
fn rust_direction_recursion_and_use_ranges_are_preserved() -> TestResult {
    let project = Project::new()?;
    let source = "pub fn target() {}\npub fn caller() { target(); caller(); }\n";
    project.write("src/main.rs", source)?;
    let report = project.build(false)?;
    assert_eq!(report.parsed_files, 1);
    let deps = project.query(Query::Deps {
        selector: selector("src/main.rs", "caller"),
    })?;
    let edges = relationships(&deps);
    assert_eq!(edges.len(), 2);
    for edge in edges {
        assert_eq!(edge.source.name, "caller");
        assert_eq!(edge.relation, "calls");
        assert_eq!(edge.resolution, "resolved");
        let name = &edge.target.as_ref().ok_or("missing target")?.name;
        assert_eq!(
            &source.as_bytes()[edge.site.span.start_byte..edge.site.span.end_byte],
            name.as_bytes()
        );
    }
    let refs = project.query(Query::Refs {
        selector: selector("src/main.rs", "target"),
    })?;
    assert_eq!(relationships(&refs).len(), 1);
    assert_eq!(relationships(&refs)[0].direction, "in");
    Ok(())
}

#[test]
fn parameter_shadowing_does_not_bind_to_a_same_named_global() -> TestResult {
    let project = Project::new()?;
    project.write(
        "api.py",
        "def target():\n    pass\n\ndef caller(target):\n    target()\n",
    )?;
    project.build(false)?;
    let deps = project.query(Query::Deps {
        selector: selector("api.py", "caller"),
    })?;
    assert!(relationships(&deps).is_empty());
    let explained = project.query(Query::Explain {
        selector: selector("api.py", "caller"),
    })?;
    assert!(relationships(&explained).iter().any(|edge| {
        edge.resolution == "unresolved"
            && edge
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("shadows"))
    }));
    Ok(())
}

#[test]
fn changed_import_targets_are_reresolved_without_reparsing_the_caller() -> TestResult {
    let project = Project::new()?;
    project.write(
        "api.ts",
        "import { target as invoke } from './dep';\nexport function caller() { invoke(); }\n",
    )?;
    project.write("dep.ts", "export function target() {}\n")?;
    project.build(false)?;
    let initial = project.query(Query::Deps {
        selector: selector("api.ts", "caller"),
    })?;
    assert_eq!(relationships(&initial).len(), 1);
    project.write("dep.ts", "export function other() {}\n")?;
    let update = project.build(false)?;
    assert_eq!(update.parsed_files, 1);
    assert_eq!(update.coverage.reused_files, 1);
    let missing = project.query(Query::Deps {
        selector: selector("api.ts", "caller"),
    })?;
    assert!(relationships(&missing).is_empty());
    project.write("dep.ts", "export function target() {}\n")?;
    project.build(false)?;
    let restored = project.query(Query::Deps {
        selector: selector("api.ts", "caller"),
    })?;
    assert_eq!(relationships(&restored).len(), 1);
    Ok(())
}

#[test]
fn parse_failure_preserves_previous_generation_and_stale_access_is_explicit() -> TestResult {
    let project = Project::new()?;
    project.write("src/main.rs", "pub fn stable() {}\n")?;
    let before = project.build(false)?;
    project.write("src/main.rs", "pub fn broken( {\n")?;
    assert!(project.build(false).is_err());
    let cached = project.query(Query::Symbols {
        file: PathBuf::from("src/main.rs"),
    })?;
    let metadata = cached.metadata.ok_or("missing metadata")?;
    assert_eq!(metadata.generation, Some(before.generation));
    assert_eq!(metadata.freshness, "not_checked");
    assert!(
        cached
            .rows
            .iter()
            .any(|row| matches!(row,QueryRow::Symbol(symbol) if symbol.name=="stable"))
    );
    let mut sink = Collected::default();
    let error = project
        .engine
        .query(
            &project.options(),
            &Query::Symbols {
                file: PathBuf::from("src/main.rs"),
            },
            &QueryOptions::default(),
            &Cancellation::default(),
            &mut sink,
        )
        .err()
        .ok_or("refresh should fail")?;
    assert_eq!(error.kind, ErrorKind::Operation);
    assert!(sink.rows.is_empty());
    Ok(())
}

#[test]
fn no_index_query_does_not_initialize_a_cache() -> TestResult {
    let project = Project::new()?;
    project.write("api.py", "def sample(): pass\n")?;
    let result = project.query(Query::Symbols {
        file: PathBuf::from("api.py"),
    });
    assert_eq!(
        result.err().ok_or("missing failure")?.kind,
        ErrorKind::NotFound
    );
    assert!(!project.directory.join("cache").exists());
    Ok(())
}

#[test]
fn unchanged_build_does_not_advance_generation() -> TestResult {
    let project = Project::new()?;
    project.write("api.py", "def sample(): pass\n")?;
    let initial = project.build(false)?;
    let reused = project.build(false)?;
    assert!(!reused.changed);
    assert_eq!(reused.generation, initial.generation);
    assert_eq!(reused.parsed_files, 0);
    Ok(())
}

#[test]
fn byte_path_identity_and_symlink_root_are_not_display_string_identity() -> TestResult {
    let project = Project::new()?;
    project.write("api.py", "def sample(): pass\n")?;
    project.build(false)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link = project.directory.join("linked-project");
        symlink(&project.root, &link)?;
        let options = ProjectOptions::new(link);
        let report = project.engine.init(
            &options,
            InitOptions::default(),
            &Cancellation::default(),
            &mut |_| Ok(()),
        )?;
        assert!(!report.changed);
    }
    assert!(Path::new(&project.root).is_absolute());
    Ok(())
}
