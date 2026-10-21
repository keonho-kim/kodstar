// Purpose: manifest와 template의 명시적 파일·패키지 관계 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn manifest_file_paths_resolve_without_executing_or_fetching_dependencies() -> TestResult {
    let project = Project::new()?;
    project.write("package.json", r#"{"name":"fixture","main":"./src/main.ts","dependencies":{"local":"file:./packages/local","remote":"1.0"}}"#)?;
    project.write("packages/local/package.json", r#"{"name":"local"}"#)?;
    project.write("src/main.ts", "export function main() {}")?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("package.json", "package.json"),
    })?;
    let edges = relationships(&rows);
    assert!(edges.iter().any(|edge| {
        edge.relation == "includes"
            && edge.resolution == "resolved"
            && edge
                .target
                .as_ref()
                .is_some_and(|target| target.file == "src/main.ts")
    }));
    assert!(edges.iter().any(|edge| {
        edge.relation == "depends_on"
            && edge.resolution == "resolved"
            && edge
                .target
                .as_ref()
                .is_some_and(|target| target.file == "packages/local/package.json")
    }));
    assert!(edges.iter().any(|edge| edge.resolution == "resolved"
        && edge.external.as_deref() == Some("package:npm:remote")));
    Ok(())
}

#[test]
fn root_relative_template_references_are_reresolved_when_the_target_appears() -> TestResult {
    let project = Project::new()?;
    project.write(
        "resources/views/outer.blade.php",
        "@include('nested.child')",
    )?;
    project.build(false)?;
    let query = || Query::Explain {
        selector: selector(
            "resources/views/outer.blade.php",
            "resources/views/outer.blade.php",
        ),
    };
    let before = project.query(query())?;
    assert!(
        relationships(&before)
            .iter()
            .any(|edge| edge.relation == "template_include" && edge.resolution == "unresolved")
    );
    project.write("resources/views/nested/child.blade.php", "<div>child</div>")?;
    let update = project.build(false)?;
    assert_eq!(update.parsed_files, 1);
    let after = project.query(query())?;
    assert!(relationships(&after).iter().any(|edge| {
        edge.relation == "template_include"
            && edge.resolution == "resolved"
            && edge
                .target
                .as_ref()
                .is_some_and(|target| target.file == "resources/views/nested/child.blade.php")
    }));
    Ok(())
}

#[test]
fn file_references_cannot_escape_the_scanned_root() -> TestResult {
    let project = Project::new()?;
    project.write("package.json", r#"{"main":"../outside.ts"}"#)?;
    std::fs::write(
        project.directory.join("outside.ts"),
        "export function outside() {}",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("package.json", "package.json"),
    })?;
    let edges = relationships(&rows);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].resolution, "unresolved");
    assert!(
        edges[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("root"))
    );
    Ok(())
}
