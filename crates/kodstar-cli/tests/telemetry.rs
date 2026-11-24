// Purpose: 기본 상세 출력·명시적 활성/비활성의 stdout 분리와 원문 비공개 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test telemetry.
// Related: crates/kodstar-cli/src/telemetry.rs, crates/kodstar-cli/src/main.rs.

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn run(root: &Path, verbose: Option<bool>, args: &[&str]) -> std::io::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kodstar"));
    if let Some(verbose) = verbose {
        command.args(["--verbose", if verbose { "true" } else { "false" }]);
    }
    command
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"));
    command.output()
}

#[test]
fn default_and_explicit_verbosity_preserve_results_and_keep_source_out_of_timings() -> TestResult {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let project = root.join("project");
    fs::create_dir(&project)?;
    fs::write(
        project.join("main.go"),
        "package p\nfunc PrivateSourceMarker() {}\n",
    )?;
    let init = run(
        root,
        None,
        &[
            "init",
            project.to_str().ok_or("path")?,
            "--yes",
            "--format",
            "json",
        ],
    )?;
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let _: serde_json::Value = serde_json::from_slice(&init.stdout)?;
    let trace = String::from_utf8(init.stderr)?;
    for stage in [
        "prepare",
        "capture",
        "extract",
        "resolve",
        "analysis",
        "synchronize",
        "command",
    ] {
        assert!(trace.contains(stage), "missing {stage}: {trace}");
    }
    assert!(trace.contains("time.busy="));
    assert!(trace.contains("Capturing source changes"));
    assert!(!trace.contains("PrivateSourceMarker"));
    let source = project.join("main.go");
    let args = [
        "symbols",
        source.to_str().ok_or("path")?,
        "--root",
        project.to_str().ok_or("path")?,
        "--format",
        "json",
        "--no-refresh",
    ];
    let normal = run(root, None, &args)?;
    let enabled = run(root, Some(true), &args)?;
    let disabled = run(root, Some(false), &args)?;
    for output in [&normal, &enabled, &disabled] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(normal.stdout, enabled.stdout);
    assert_eq!(normal.stdout, disabled.stdout);
    assert!(String::from_utf8(normal.stderr)?.contains("time.busy="));
    assert!(disabled.stderr.is_empty());
    let trace = String::from_utf8(enabled.stderr)?;
    assert!(trace.contains("query") && trace.contains("rows=1"));
    assert!(!trace.contains("PrivateSourceMarker"));
    for verbose in [None, Some(true), Some(false)] {
        use std::{io::Write, process::Stdio};
        let mut command = Command::new(env!("CARGO_BIN_EXE_kodstar"));
        if let Some(verbose) = verbose {
            command.args(["--verbose", if verbose { "true" } else { "false" }]);
        }
        let mut session = command
            .args(["serve", "--stdio"])
            .current_dir(&project)
            .env("HOME", root.join("home"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let request = serde_json::json!({
            "protocol_version": 2, "id": "q", "method": "query",
            "params": {
                "query": { "kind": "symbols", "file": "main.go" },
                "options": { "refresh": "none" }
            }
        });
        let mut input = session.stdin.take().ok_or("stdin")?;
        serde_json::to_writer(&mut input, &request)?;
        input.write_all(b"\n")?;
        drop(input);
        let output = session.wait_with_output()?;
        assert!(output.status.success());
        let events: Vec<serde_json::Value> = String::from_utf8(output.stdout)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        assert!(
            events
                .iter()
                .any(|event| event["type"] == "complete" && event["exit_code"] == 0)
        );
        assert!(events.iter().any(|event| event["type"] == "progress"));
        let trace = String::from_utf8(output.stderr)?;
        assert_eq!(trace.contains("query{rows=1"), verbose != Some(false));
        if verbose == Some(false) {
            assert!(trace.is_empty());
        }
    }
    directory.close()?;
    Ok(())
}

#[test]
fn verbosity_requires_a_boolean_before_creating_application_data() -> TestResult {
    let directory = tempfile::tempdir()?;
    for arguments in [
        vec!["languages", "--verbose", "invalid"],
        vec!["languages", "--verbose"],
    ] {
        let output = run(directory.path(), None, &arguments)?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr)?.contains("--verbose"));
        assert!(!directory.path().join("home").exists());
    }
    for verbose in [None, Some(true), Some(false)] {
        let help = run(directory.path(), verbose, &["--help"])?;
        assert!(help.status.success());
        assert!(help.stderr.is_empty());
    }
    directory.close()?;
    Ok(())
}
