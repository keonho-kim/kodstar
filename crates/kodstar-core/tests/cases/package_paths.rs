// Purpose: workspace·package 공개 경계·조건과 source entry의 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn pnpm_workspace_exclusions_and_platform_variants_preserve_uncertainty() -> TestResult {
    let project = Project::new()?;
    project.write(
        "pnpm-workspace.yaml",
        "packages:\n  - 'packages/*'\n  - '!packages/excluded'\n",
    )?;
    project.write(
        "packages/api/package.json",
        r#"{"name":"api","exports":{"./*":"./src/*.tsx"}}"#,
    )?;
    project.write(
        "packages/api/src/nav.web.tsx",
        "export function target() {}",
    )?;
    project.write(
        "packages/api/src/nav.native.tsx",
        "export function target() {}",
    )?;
    project.write(
        "packages/excluded/package.json",
        r#"{"name":"excluded","main":"./index.ts"}"#,
    )?;
    project.write("packages/excluded/index.ts", "export function hidden() {}")?;
    project.write("main.ts","import { target } from 'api/nav'; import { hidden } from 'excluded'; function caller() { target(); hidden(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("main.ts", "caller"),
    })?;
    let platform: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.target.as_ref().is_some_and(|t| t.name == "target"))
        .collect();
    assert_eq!(platform.len(), 2, "{:?}", rows.rows);
    assert!(platform.iter().all(|e| e.resolution == "ambiguous"));
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "hidden"))
    );
    Ok(())
}

#[test]
fn workspace_exports_prefer_source_and_ignore_unlisted_same_named_packages() -> TestResult {
    let project = Project::new()?;
    project.write(
        "package.json",
        r#"{"private":true,"workspaces":["packages/*"]}"#,
    )?;
    project.write("packages/api/package.json",r#"{"name":"@acme/api","exports":{".":{"import":"./dist/index.js","source":"./src/index.ts"}}}"#)?;
    project.write("packages/api/src/index.ts", "export function target() {}")?;
    project.write(
        "outside/package.json",
        r#"{"name":"@acme/api","main":"./index.ts"}"#,
    )?;
    project.write("outside/index.ts", "export function target() {}")?;
    project.write(
        "main.ts",
        "import { target } from '@acme/api'; function caller() { target(); }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.ts", "caller"),
    })?;
    assert_eq!(relationships(&rows).len(), 1, "{:?}", rows.rows);
    assert_eq!(
        relationships(&rows)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .file,
        "packages/api/src/index.ts"
    );
    Ok(())
}

#[test]
fn package_subpaths_and_private_imports_respect_export_boundaries() -> TestResult {
    let project = Project::new()?;
    project.write(
        "package.json",
        r##"{"workspaces":["packages/*"],"imports":{"#local":"./local.ts"}}"##,
    )?;
    project.write("local.ts", "export function target() {}")?;
    project.write("packages/api/package.json",r#"{"name":"api","main":"./src/index.ts","exports":{"./features/*":{"source":"./src/features/*.ts"}}}"#)?;
    project.write(
        "packages/api/src/features/item.ts",
        "export function target() {}",
    )?;
    project.write("packages/api/src/index.ts", "export function hidden() {}")?;
    project.write("main.ts","import { target as feature } from 'api/features/item'; import { target as local } from '#local'; import { hidden } from 'api'; function caller() { feature(); local(); hidden(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.ts", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 2, "{:?}", rows.rows);
    assert!(
        !calls
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "hidden"))
    );
    Ok(())
}

#[test]
fn explicit_local_dependencies_use_source_twins_and_reject_escaping_exports() -> TestResult {
    let project = Project::new()?;
    project.write(
        "package.json",
        r#"{"dependencies":{"local":"file:./library"}}"#,
    )?;
    project.write(
        "library/package.json",
        r#"{"name":"local","exports":{".":"./dist/index.js"}}"#,
    )?;
    project.write("library/src/index.ts", "export function target() {}")?;
    project.write(
        "main.ts",
        "import { target } from 'local'; function caller() { target(); }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.ts", "caller"),
    })?;
    assert!(relationships(&rows).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.file == "library/src/index.ts")
    }));
    project.write("secret.ts", "export function target() {}")?;
    project.write(
        "library/package.json",
        r#"{"name":"local","exports":{".":"../secret.ts"}}"#,
    )?;
    project.build(false)?;
    assert!(
        relationships(&project.query(Query::Deps {
            selector: selector("main.ts", "caller")
        })?)
        .is_empty()
    );
    Ok(())
}
