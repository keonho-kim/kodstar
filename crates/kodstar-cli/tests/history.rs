// Purpose: 실제 CLI의 초기화·자동 이력·복원·브랜치 통합 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test history.
// Related: crates/kodstar-cli/tests/support/mod.rs.

mod support;
use serde_json::Value;
use support::*;

fn json(project: &Project, args: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
    let mut args = args.to_vec();
    args.extend(["--format", "json"]);
    let result = project.run(&args)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(serde_json::from_slice(&result.stdout)?)
}

#[test]
fn init_and_existing_queries_create_history_without_explicit_commits() -> TestResult {
    let p = Project::new()?;
    p.write("fn first() {}")?;
    let initial = json(&p, &["init"])?;
    assert_eq!(initial["history"]["checkpoint"], "c1");
    let upgraded = json(&p, &["history", "upgrade"])?;
    assert_eq!(upgraded["results"][0]["operation"], "upgrade");
    assert_eq!(upgraded["history"]["checkpoint"], "c1");
    json(&p, &["search", "first"])?;
    assert_eq!(
        json(&p, &["history", "--no-refresh"])?["results"]
            .as_array()
            .ok_or("rows")?
            .len(),
        1
    );
    p.write("fn second() {}")?;
    assert_eq!(
        json(&p, &["search", "second"])?["history"]["checkpoint"],
        "c2"
    );
    json(&p, &["history", "save", "milestone"])?;
    let diff = json(
        &p,
        &["history", "diff", "--from", "c1", "--to", "milestone"],
    )?;
    assert_eq!(diff["history"]["comparison"]["from"], "c1");
    assert!(
        diff["results"]
            .as_array()
            .ok_or("diff rows")?
            .iter()
            .any(|row| row["kind"] == "diff_line")
    );
    json(&p, &["history", "restore", "c1"])?;
    assert_eq!(
        std::fs::read_to_string(p.root.join("main.rs"))?,
        "fn first() {}"
    );
    let past = json(&p, &["symbols", "main.rs", "--rev", "milestone"])?;
    assert_eq!(past["results"][0]["name"], "second");
    assert_eq!(past["freshness"], "checkpoint");
    assert_eq!(p.run(&["build"])?.status.code(), Some(2));
    Ok(())
}

#[test]
fn branch_commands_protect_edits_and_change_impact_uses_both_states() -> TestResult {
    let p = Project::new()?;
    p.write("fn target() {}\nfn caller() { target(); }")?;
    json(&p, &["init"])?;
    json(&p, &["branch", "create", "experiment"])?;
    p.write("fn caller() { target(); }")?;
    let impact = json(&p, &["impact", "--since", "c1"])?;
    assert!(
        impact["results"]
            .as_array()
            .ok_or("impact")?
            .iter()
            .any(|row| row["checkpoint"] == "c1"
                && row["relationship"]["source"]["name"] == "caller")
    );
    json(&p, &["branch", "switch", "experiment"])?;
    assert!(std::fs::read_to_string(p.root.join("main.rs"))?.contains("fn target"));
    assert_eq!(
        p.run(&["branch", "delete", "experiment"])?.status.code(),
        Some(2)
    );
    assert_eq!(
        p.run(&["history", "restore", "c1", "--no-refresh"])?
            .status
            .code(),
        Some(2)
    );
    Ok(())
}
