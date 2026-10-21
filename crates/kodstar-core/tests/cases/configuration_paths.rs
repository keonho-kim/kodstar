// Purpose: JSONC 경로 설정·alias 우선순위·상속과 증분 일관성 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn configuration_links_preserve_original_unicode_byte_positions() -> TestResult {
    let project = Project::new()?;
    let source = "// 한글\n{\"extends\":\"./configs/base\",\"ignored\":\",}\",}";
    project.write("tsconfig.json", source)?;
    project.write("configs/base.json", "{}")?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("tsconfig.json", "tsconfig.json"),
    })?;
    let edge = relationships(&rows)
        .into_iter()
        .find(|e| e.relation == "includes")
        .ok_or("configuration include")?;
    assert_eq!(
        edge.site.span.start_byte,
        source.find("\"./configs/base\"").ok_or("link")?
    );
    assert_eq!(
        &source[edge.site.span.start_byte..edge.site.span.end_byte],
        "\"./configs/base\""
    );
    Ok(())
}

#[test]
fn removing_references_retires_only_owned_configuration_facts() -> TestResult {
    let project = Project::new()?;
    project.write("tsconfig.json", r#"{"extends":"./configs/base"}"#)?;
    project.write(
        "configs/base.json",
        r#"{"compilerOptions":{"paths":{"@api":["../api"]}}}"#,
    )?;
    project.write("api.ts", "export function target() {}")?;
    project.write(
        "main.ts",
        "import { target } from '@api'; function caller() { target(); }",
    )?;
    project.build(false)?;
    project.write("tsconfig.json", "{}")?;
    project.build(false)?;
    assert!(project.root.join("configs/base.json").is_file());
    assert!(
        relationships(&project.query(Query::Deps {
            selector: selector("main.ts", "caller")
        })?)
        .is_empty()
    );
    assert!(!project.build(false)?.changed);
    project.build(true)?;
    assert!(
        relationships(&project.query(Query::Deps {
            selector: selector("main.ts", "caller")
        })?)
        .is_empty()
    );
    Ok(())
}

#[test]
fn excessive_configuration_nesting_rolls_back_the_previous_generation() -> TestResult {
    let project = Project::new()?;
    project.write("tsconfig.json", "{}")?;
    project.write("main.ts", "function target() {}")?;
    let first = project.build(false)?;
    project.write(
        "tsconfig.json",
        &format!("{{\"ignored\":{}0{}}}", "[".repeat(513), "]".repeat(513)),
    )?;
    assert_eq!(
        project.build(false).err().ok_or("limit")?.kind,
        ErrorKind::Limit
    );
    let cached = project.query(Query::Symbols {
        file: "main.ts".into(),
    })?;
    assert_eq!(
        cached.metadata.ok_or("metadata")?.generation,
        Some(first.generation)
    );
    Ok(())
}

#[test]
fn jsonc_aliases_use_base_url_specificity_and_ordered_fallbacks() -> TestResult {
    let project = Project::new()?;
    project.write("tsconfig.json",r#"{ // paths may follow baseUrl in either order
      "compilerOptions": { "paths": { "@app/*": ["missing/*", "generic/*",], "@app/api": ["exact/api"], "@app/deep/*": ["specific/*"] }, "baseUrl": "./src", },
    }"#)?;
    for path in [
        "src/generic/api.ts",
        "src/exact/api.ts",
        "src/specific/item.ts",
        "src/base.ts",
    ] {
        project.write(path, "export function target() {}")?;
    }
    project.write("src/main.ts","import { target as exact } from '@app/api'; import { target as specific } from '@app/deep/item'; import { target as base } from 'base'; function caller() { exact(); specific(); base(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("src/main.ts", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 3, "{:?}", rows.rows);
    for file in ["src/exact/api.ts", "src/specific/item.ts", "src/base.ts"] {
        assert!(
            calls
                .iter()
                .any(|e| e.target.as_ref().is_some_and(|t| t.file == file))
        );
    }
    Ok(())
}

#[test]
fn inherited_aliases_from_arbitrary_config_names_reresolve_without_parsing_callers() -> TestResult {
    let project = Project::new()?;
    project.write("tsconfig.json", r#"{"extends":"./configs/base"}"#)?;
    project.write(
        "configs/base.json",
        r#"{"compilerOptions":{"baseUrl":"../src","paths":{"@api":["one"]}}}"#,
    )?;
    project.write("src/one.ts", "export function target() {}")?;
    project.write("src/two.ts", "export function target() {}")?;
    project.write(
        "src/main.ts",
        "import { target } from '@api'; function caller() { target(); }",
    )?;
    project.build(false)?;
    let query = || Query::Deps {
        selector: selector("src/main.ts", "caller"),
    };
    let first = project.query(query())?;
    assert!(
        relationships(&first)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.file == "src/one.ts"))
    );
    project.write(
        "configs/base.json",
        r#"{"compilerOptions":{"baseUrl":"../src","paths":{"@api":["two"]}}}"#,
    )?;
    let report = project.build(false)?;
    assert_eq!(report.parsed_files, 1);
    let incremental = project.query(query())?;
    assert!(
        relationships(&incremental)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.file == "src/two.ts"))
    );
    project.build(true)?;
    let full = project.query(query())?;
    assert_eq!(
        relationships(&incremental)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .logical_key,
        relationships(&full)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .logical_key
    );
    assert!(!project.build(false)?.changed);
    Ok(())
}

#[test]
fn bare_imports_are_not_guessed_from_same_named_project_files() -> TestResult {
    let project = Project::new()?;
    project.write("api.ts", "export function target() {}")?;
    project.write(
        "main.ts",
        "import { target } from 'api'; function caller() { target(); }",
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
