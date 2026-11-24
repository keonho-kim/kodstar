// Purpose: 잘못된 설정의 격리·가까운 설정 경계·복구 후 증분 일치 검증.
// Pattern: Differential Regression Test.
// Usage: engine 통합 검사에서 원문 실행 없이 잘못된 설정을 교체.
// Related: crates/kodstar-core/src/application/analysis/configuration_input.rs, crates/kodstar-core/src/application/analysis/resolve/javascript/configurations.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;

#[test]
fn invalid_nearest_configuration_blocks_ancestor_aliases_but_keeps_relative_imports() -> TestResult
{
    for invalid in [
        "{\"compilerOptions\":",
        "[]",
        r#"{"compilerOptions":{"paths":{"api":"wrong"}}}"#,
    ] {
        let p = Project::new()?;
        p.write(
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"api":["./wrong.ts"]}}}"#,
        )?;
        p.write("wrong.ts", "export function target() {}")?;
        p.write("sub/right.ts", "export function target() {}")?;
        p.write("sub/tsconfig.json", invalid)?;
        p.write(
            "sub/caller.ts",
            "import { target } from 'api'; export function caller() { target(); }",
        )?;
        p.write(
            "sub/local.ts",
            "import { target } from './right'; export function local() { target(); }",
        )?;
        let failed = p.build(false)?;
        assert_eq!(failed.coverage.configuration_incomplete_files, 1);
        assert_eq!(failed.coverage.failed_files, 1);
        assert_eq!(unchanged(&p)?.generation, failed.generation);
        let rows = p.query(Query::Deps {
            selector: selector("sub/caller.ts", "caller"),
        })?;
        assert!(!rows.summary.as_ref().ok_or("summary")?.complete);
        assert!(relationships(&rows).iter().all(|r| r.target.is_none()));
        let rows = p.query(Query::Deps {
            selector: selector("sub/local.ts", "local"),
        })?;
        assert!(
            relationships(&rows)
                .iter()
                .any(|r| r.target.as_ref().is_some_and(|s| s.file == "sub/right.ts"))
        );
        assert_eq!(
            normalized_index(&failed)?,
            normalized_index(&p.build(true)?)?
        );
        p.write(
            "sub/tsconfig.json",
            r#"{"compilerOptions":{"paths":{"api":["./right.ts"]}}}"#,
        )?;
        let repaired = p.build(false)?;
        assert_eq!(repaired.coverage.failed_files, 0);
        assert_eq!(repaired.coverage.configuration_incomplete_files, 0);
        let rows = p.query(Query::Deps {
            selector: selector("sub/caller.ts", "caller"),
        })?;
        assert!(
            relationships(&rows)
                .iter()
                .any(|r| r.target.as_ref().is_some_and(|s| s.file == "sub/right.ts"))
        );
        assert_eq!(
            normalized_index(&repaired)?,
            normalized_index(&p.build(true)?)?
        );
    }
    Ok(())
}

#[test]
fn invalid_referenced_configuration_is_checked_before_any_facts_are_persisted() -> TestResult {
    let p = Project::new()?;
    p.write("tsconfig.json", r#"{"extends":"./settings/base.json"}"#)?;
    p.write(
        "settings/base.json",
        r#"{"compilerOptions":{"baseUrl":"..","paths":{"api":"invalid"}}}"#,
    )?;
    p.write("api.ts", "export function target() {}")?;
    p.write(
        "caller.ts",
        "import { target } from 'api'; export function caller() { target(); }",
    )?;
    let failed = p.build(false)?;
    assert_eq!(failed.coverage.failed_files, 1);
    assert_eq!(failed.coverage.configuration_incomplete_files, 1);
    assert_eq!(unchanged(&p)?.generation, failed.generation);
    let rows = p.query(Query::Deps {
        selector: selector("caller.ts", "caller"),
    })?;
    assert!(relationships(&rows).iter().all(|r| r.target.is_none()));
    assert_eq!(
        normalized_index(&failed)?,
        normalized_index(&p.build(true)?)?
    );
    p.write(
        "settings/base.json",
        r#"{"compilerOptions":{"paths":{"api":["../api.ts"]}}}"#,
    )?;
    let repaired = p.build(false)?;
    assert_eq!(repaired.coverage.configuration_incomplete_files, 0);
    let rows = p.query(Query::Deps {
        selector: selector("caller.ts", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|s| s.file == "api.ts"))
    );
    assert_eq!(
        normalized_index(&repaired)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

fn unchanged(p: &Project) -> Result<InitReport> {
    p.engine.init(
        &p.options(),
        InitOptions {
            full: false,
            verify: false,
            path: None,
        },
        &Cancellation::default(),
        &mut |_| Ok(()),
    )
}

#[test]
fn removing_an_invalid_reference_retires_only_its_owned_analysis_state() -> TestResult {
    let p = Project::new()?;
    p.write("tsconfig.json", r#"{"extends":"./settings/base.json"}"#)?;
    p.write("settings/base.json", "{ broken configuration")?;
    p.write("api.ts", "export function target() {}")?;
    assert_eq!(p.build(false)?.coverage.failed_files, 1);
    p.write("tsconfig.json", "{}")?;
    let removed = p.build(false)?;
    assert_eq!(removed.coverage.failed_files, 0);
    assert_eq!(removed.coverage.configuration_incomplete_files, 0);
    assert!(p.root.join("settings/base.json").is_file());
    assert_eq!(
        normalized_index(&removed)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn oversized_referenced_configuration_preserves_a_usable_graph() -> TestResult {
    let p = Project::new()?;
    p.write("tsconfig.json", r#"{"extends":"./settings/base.json"}"#)?;
    p.write(
        "settings/base.json",
        &format!(r#"{{"ignored":"{}"}}"#, "x".repeat(2048)),
    )?;
    p.write("api.ts", "export function target() {}")?;
    let config = p.directory.join("settings.toml");
    std::fs::write(&config, "max_file_bytes = 1024\n")?;
    let mut options = p.options();
    options.config = Some(config);
    let report = p.engine.init(
        &options,
        InitOptions {
            full: false,
            verify: false,
            path: None,
        },
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    assert_eq!(report.coverage.failed_files, 1);
    assert_eq!(report.coverage.configuration_incomplete_files, 1);
    let mut rows = Collected::default();
    p.engine.query(
        &options,
        &Query::Symbols {
            file: "api.ts".into(),
        },
        &QueryOptions {
            refresh: Refresh::None,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut rows,
    )?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "target"))
    );
    Ok(())
}
