// Purpose: 실제 CLI의 분석·조회·출력·실패 계약 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test commands.
// Related: crates/kodstar-cli/src/run.rs, crates/kodstar-cli/tests/support/mod.rs.

mod support;
use serde_json::Value;
use support::*;

#[test]
fn build_and_queries_stream_structured_results_and_refresh() -> TestResult {
    let project = Project::new()?;
    project.write("fn target() {}\nfn caller() { target(); }\n")?;
    let build = project.run(&["build", "--format", "json"])?;
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let built: Value = serde_json::from_slice(&build.stdout)?;
    assert_eq!(built["schema_version"], 1);
    assert_eq!(built["complete"], true);
    assert_eq!(built["generation"], 1);
    let deps = project.run(&["deps", "main.rs", "caller", "--format", "jsonl"])?;
    assert!(
        deps.status.success(),
        "{}",
        String::from_utf8_lossy(&deps.stderr)
    );
    let events: Vec<Value> = String::from_utf8(deps.stdout)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(events[0]["type"], "metadata");
    assert_eq!(events[0]["freshness"], "metadata_checked");
    assert!(
        events
            .iter()
            .any(|row| row["target"]["name"] == "target" && row["direction"] == "out")
    );
    assert_eq!(events.last().ok_or("missing summary")?["complete"], true);
    project.write("fn renamed() {}\nfn caller() { renamed(); }\n")?;
    let refreshed = project.run(&["deps", "main.rs", "caller", "--verify", "--format", "json"])?;
    assert!(refreshed.status.success());
    let updated: Value = serde_json::from_slice(&refreshed.stdout)?;
    assert_eq!(updated["generation"], 2);
    assert_eq!(updated["freshness"], "content_verified");
    assert_eq!(updated["results"][0]["target"]["name"], "renamed");
    Ok(())
}

#[test]
fn output_limit_returns_complete_json_with_a_truncation_exit_code() -> TestResult {
    let project = Project::new()?;
    project.write("fn one() {} fn two() {}")?;
    assert!(project.run(&["build"])?.status.success());
    let output = project.run(&["symbols", "main.rs", "--limit", "1", "--format", "json"])?;
    assert_eq!(output.status.code(), Some(3));
    let value: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value["complete"], false);
    assert_eq!(value["truncated"], true);
    assert_eq!(
        value["results"].as_array().ok_or("missing results")?.len(),
        1
    );
    Ok(())
}

#[test]
fn meaningful_options_and_values_are_rejected_before_analysis() -> TestResult {
    let project = Project::new()?;
    for args in [
        vec!["symbols", "main.rs", "--no-refresh", "--verify"],
        vec!["deps", "main.rs", "f", "--direction", "in"],
        vec!["refs", "main.rs", "f", "--direction", "out"],
        vec!["related", "main.rs", "f", "--depth", "2", "--transitive"],
        vec!["related", "main.rs", "f", "--relation", "unknown"],
        vec!["build", "--memory-budget", "12garbage"],
        vec!["build", "--jobs", "0"],
        vec!["build", "--sites"],
    ] {
        let output = project.run(&args)?;
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    assert!(!project.home.join(".kodstar").exists());
    Ok(())
}

#[test]
fn missing_index_reports_build_guidance_without_creating_cache() -> TestResult {
    let project = Project::new()?;
    let output = project.run(&["symbols", "main.rs"])?;
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)?.contains("build"));
    assert!(!project.home.join(".kodstar").exists());
    Ok(())
}

#[test]
fn text_tsv_status_and_languages_have_real_results() -> TestResult {
    let project = Project::new()?;
    project.write("fn target() {} fn caller() { target(); }")?;
    assert!(project.run(&["build"])?.status.success());
    let text = project.run(&["deps", "main.rs", "caller"])?;
    assert!(text.status.success());
    assert!(String::from_utf8(text.stdout)?.contains("target"));
    let tsv = project.run(&["symbols", "main.rs", "--format", "tsv"])?;
    assert!(tsv.status.success());
    let tsv = String::from_utf8(tsv.stdout)?;
    assert!(tsv.starts_with("record\t"));
    assert!(tsv.lines().any(|line| line.starts_with("result\tsymbol\t")));
    assert!(
        tsv.lines()
            .last()
            .ok_or("missing TSV summary")?
            .starts_with("summary\t")
    );
    let status = project.run(&["doctor", "--format", "json"])?;
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout)?;
    assert_eq!(status["results"][0]["integrity"], "ok");
    let languages = project.run(&["languages", "--format", "json"])?;
    assert!(languages.status.success());
    let languages: Value = serde_json::from_slice(&languages.stdout)?;
    assert_eq!(
        languages["results"]
            .as_array()
            .ok_or("missing languages")?
            .len(),
        25
    );
    Ok(())
}

#[test]
fn cache_commands_stream_candidates_and_do_not_delete_sources() -> TestResult {
    let project = Project::new()?;
    project.write("fn target() {}")?;
    assert!(project.run(&["build"])?.status.success());
    let list = project.run(&["cache", "list", "--format", "json"])?;
    assert!(list.status.success());
    let list: Value = serde_json::from_slice(&list.stdout)?;
    assert_eq!(list["results"].as_array().ok_or("missing caches")?.len(), 1);
    let clean = project.run(&["cache", "clean", "--format", "json"])?;
    assert!(clean.status.success());
    let clean: Value = serde_json::from_slice(&clean.stdout)?;
    assert_eq!(clean["results"][0]["action"], "removed");
    assert!(project.root.join("main.rs").exists());
    assert_eq!(project.run(&["symbols", "main.rs"])?.status.code(), Some(4));
    Ok(())
}

#[test]
fn positional_build_root_must_agree_with_explicit_root() -> TestResult {
    let project = Project::new()?;
    project.write("fn target() {}")?;
    let good = project.run(&["build", ".", "--root", ".", "--format", "json"])?;
    assert!(good.status.success());
    let bad = project.run(&["build", ".", "--root", ".."])?;
    assert_eq!(bad.status.code(), Some(2));
    assert!(bad.stdout.is_empty());
    Ok(())
}
