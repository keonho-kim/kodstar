// Purpose: 프로젝트 개요·순위 검색·방향 전환 경로의 CLI 계약 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test graph_navigation.
// Related: crates/kodstar-cli/tests/support/mod.rs, crates/kodstar-core/src/application/query/mod.rs.

mod support;

use serde_json::Value;
use std::fs;
use support::{Project, TestResult};

fn json(project: &Project, args: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
    let output = project.run(args)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn overview_ranked_search_and_undirected_path_use_indexed_evidence() -> TestResult {
    let project = Project::new()?;
    project.write("fn main_fixture() {}\n")?;
    for directory in ["left", "right", "shared"] {
        fs::create_dir(project.root.join(directory))?;
    }
    for (path, source) in [
        ("go.mod", "module example.test/navigation\n"),
        (
            "shared/storage.go",
            "package shared\nfunc StoreOrder() {}\n",
        ),
        (
            "left/left.go",
            "package left\nimport \"example.test/navigation/shared\"\nfunc Checkout() { shared.StoreOrder() }\n",
        ),
        (
            "right/right.go",
            "package right\nimport \"example.test/navigation/shared\"\nfunc Retry() { shared.StoreOrder() }\n",
        ),
    ] {
        fs::write(project.root.join(path), source)?;
    }
    json(&project, &["init", "--format", "json"])?;
    let overview = json(&project, &["overview", "--all", "--format", "json"])?;
    let rows = overview["results"].as_array().ok_or("overview rows")?;
    assert!(rows.iter().any(|row| row["kind"] == "module_dependency"));
    assert!(rows.iter().any(|row| row["kind"] == "hub"
        && row["symbol"]["name"] == "StoreOrder"
        && row["incoming"] == 2));

    let ranked = json(
        &project,
        &[
            "search",
            "shared store order",
            "--ranked",
            "--format",
            "json",
        ],
    )?;
    assert_eq!(ranked["results"][0]["name"], "StoreOrder");
    assert!(
        ranked["results"]
            .as_array()
            .ok_or("ranked rows")?
            .iter()
            .any(|row| row["relation"] == "calls" && row["target"]["name"] == "StoreOrder")
    );
    let selectors = ["left/left.go::Checkout", "right/right.go::Retry"];
    let result = json(
        &project,
        &[
            "path",
            selectors[0],
            selectors[1],
            "--relation",
            "calls",
            "--format",
            "json",
        ],
    )?;
    assert_eq!(
        result["results"].as_array().ok_or("directed rows")?.len(),
        0
    );
    let result = json(
        &project,
        &[
            "path",
            selectors[0],
            selectors[1],
            "--undirected",
            "--relation",
            "calls",
            "--format",
            "json",
        ],
    )?;
    let hops = result["results"].as_array().ok_or("path rows")?;
    assert_eq!(hops.len(), 2);
    assert_eq!(hops[0]["direction"], "out");
    assert_eq!(hops[1]["direction"], "in");
    assert_eq!(hops[0]["target"]["name"], "StoreOrder");
    assert_eq!(hops[1]["target"]["name"], "StoreOrder");

    let bounded = project.run(&[
        "path",
        selectors[0],
        selectors[1],
        "--undirected",
        "--relation",
        "calls",
        "--max-nodes",
        "2",
        "--format",
        "json",
    ])?;
    assert_eq!(bounded.status.code(), Some(3));
    let bounded: Value = serde_json::from_slice(&bounded.stdout)?;
    assert_eq!(bounded["truncated"], true);
    assert_eq!(bounded["reasons"], serde_json::json!(["visit_limit"]));
    Ok(())
}

#[test]
fn overview_identifies_cross_package_dependencies_and_import_cycles() -> TestResult {
    let project = Project::new()?;
    project.write("fn fixture() {}\n")?;
    for directory in ["app", "library"] {
        fs::create_dir(project.root.join(directory))?;
    }
    for (path, source) in [
        ("go.work", "go 1.25\nuse (\n ./app\n ./library\n)\n"),
        ("app/go.mod", "module example.test/app\n"),
        ("library/go.mod", "module example.test/library\n"),
        (
            "app/api.go",
            "package app\nimport \"example.test/library\"\nfunc App() { library.Run() }\n",
        ),
        (
            "library/lib.go",
            "package library\nimport \"example.test/app\"\nfunc Run() { app.App() }\n",
        ),
    ] {
        fs::write(project.root.join(path), source)?;
    }
    json(&project, &["init", "--format", "json"])?;
    let result = json(&project, &["overview", "--all", "--format", "json"])?;
    let rows = result["results"].as_array().ok_or("overview rows")?;
    assert!(
        rows.iter().any(|row| row["kind"] == "package_dependency"
            && row["from"]["path"] == "app"
            && row["to"]["path"] == "library"),
        "{result:#}"
    );
    assert!(
        rows.iter()
            .any(|row| row["kind"] == "import_cycle" && row["files"] == 2),
        "{result:#}"
    );
    let bounded = project.run(&["overview", "--max-nodes", "1", "--format", "json"])?;
    assert_eq!(bounded.status.code(), Some(3));
    let bounded: Value = serde_json::from_slice(&bounded.stdout)?;
    assert_eq!(bounded["visited_count"], 1);
    assert_eq!(
        bounded["reasons"],
        serde_json::json!(["overview_file_limit"])
    );
    Ok(())
}

#[test]
fn graph_diff_reports_retargeting_in_an_unchanged_caller() -> TestResult {
    let project = Project::new()?;
    project.write("mod one; mod two; mod routing; use routing::run; fn caller() { run(); }\n")?;
    fs::write(project.root.join("one.rs"), "pub fn first() {}\n")?;
    fs::write(project.root.join("two.rs"), "pub fn second() {}\n")?;
    fs::write(
        project.root.join("routing.rs"),
        "pub use crate::one::first as run;\n",
    )?;
    json(&project, &["init", "--format", "json"])?;
    json(&project, &["history", "save", "before", "--format", "json"])?;
    fs::write(
        project.root.join("routing.rs"),
        "pub use crate::two::second as run;\n",
    )?;
    json(&project, &["history", "save", "after", "--format", "json"])?;
    let report = json(
        &project,
        &[
            "history", "diff", "--graph", "--from", "before", "--to", "after", "--format", "json",
        ],
    )?;
    let comparison = &report["history"]["comparison"];
    assert_eq!(comparison["from"], "c1");
    assert_eq!(comparison["to"], "c2");
    assert!(comparison["before_coverage"].is_object());
    assert!(comparison["after_coverage"].is_object());
    let rows = report["results"].as_array().ok_or("graph diff rows")?;
    assert!(
        rows.iter().any(|row| {
            row["kind"] == "graph_change"
                && row["change"] == "retargeted"
                && row["before"]["site"]["file"] == "main.rs"
                && row["before"]["relation"] == "calls"
                && row["before"]["target"]["file"] == "one.rs"
                && row["after"]["target"]["file"] == "two.rs"
        }),
        "{report:#}"
    );
    // An unrelated historical query can replace the analysis slot while the comparison stays reusable.
    json(
        &project,
        &["symbols", "main.rs", "--rev", "before", "--format", "json"],
    )?;
    let repeated = json(
        &project,
        &[
            "history", "diff", "--graph", "--from", "before", "--to", "after", "--all", "--format",
            "json",
        ],
    )?;
    assert_eq!(report["generation"], repeated["generation"]);
    assert_eq!(report["results"], repeated["results"]);

    let reverse = json(
        &project,
        &[
            "history", "diff", "--graph", "--from", "after", "--to", "before", "--format", "json",
        ],
    )?;
    assert!(
        reverse["results"]
            .as_array()
            .ok_or("reverse changes")?
            .iter()
            .any(|row| row["change"] == "retargeted"
                && row["before"]["target"]["file"] == "two.rs"
                && row["after"]["target"]["file"] == "one.rs")
    );
    Ok(())
}

#[test]
fn explanation_links_document_evidence_from_the_queried_checkpoint() -> TestResult {
    let project = Project::new()?;
    project.write("// ADR: docs/design.md#review\nfn decision() {\n    let _ = \"RFC: docs/imagined.md\";\n}\n")?;
    fs::create_dir(project.root.join("docs"))?;
    let path = project.root.join("docs/design.md");
    fs::write(&path, "# Decision\n\n## Review\n")?;
    json(&project, &["init", "--format", "json"])?;
    fs::write(&path, "# Decision\n\nNotes\n\n## Review\n")?;
    for (flags, line) in [
        (&["--no-refresh"][..], 3),
        (&[][..], 5),
        (&["--rev", "c1"][..], 3),
    ] {
        let mut args = vec!["explain", "main.rs", "decision", "--format", "json"];
        args.extend(flags);
        let result = json(&project, &args)?;
        let docs: Vec<_> = result["results"]
            .as_array()
            .ok_or("explain rows")?
            .iter()
            .filter(|row| row["kind"] == "documentation_reference")
            .collect();
        assert_eq!(docs.len(), 1, "{result:#}");
        assert_eq!(docs[0]["document_kind"], "adr");
        assert_eq!(docs[0]["source"]["span"]["line"], 1);
        assert_eq!(docs[0]["target"]["file"], "docs/design.md");
        assert_eq!(docs[0]["target"]["span"]["line"], line);
    }
    Ok(())
}
