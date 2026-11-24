// Purpose: 기본 진행·상세 출력 비활성 시의 native 경고와 JSON stdout 분리 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test progress.
// Related: crates/kodstar-cli/src/output/progress.rs, crates/kodstar-cli/src/run.rs.

use std::{fs, process::Command};
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn redirected_progress_is_visible_and_json_stdout_remains_parseable() -> TestResult {
    let d = tempfile::tempdir()?;
    let project = d.path().join("project");
    fs::create_dir(&project)?;
    fs::write(
        project.join("main.cpp"),
        "int leaf(){return 1;}int caller(){return leaf();}",
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_kodstar"))
        .args([
            "init",
            project.to_str().ok_or("path")?,
            "--yes",
            "--format",
            "json",
        ])
        .env("HOME", d.path().join("home"))
        .env("PATH", "")
        .env("TMPDIR", d.path())
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value["complete"], true);
    let progress = String::from_utf8(output.stderr)?;
    for step in [
        "Capturing source changes",
        "Parsing and analyzing",
        "Clang semantic analysis",
        "Resolving relationships",
        "Publishing index",
    ] {
        assert!(progress.contains(step), "missing {step}: {progress}");
    }
    let preparing = progress
        .find("Preparing name and type lookup")
        .ok_or("lookup preparation progress missing")?;
    let resolving = progress
        .find("Resolving relationships")
        .ok_or("relationship progress missing")?;
    assert!(preparing < resolving, "{progress}");
    fs::write(
        project.join("main.cpp"),
        "#include \"not-installed.h\"\nint caller(){return 1;}",
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_kodstar"))
        .args([
            "symbols",
            "--verbose",
            "false",
            project.join("main.cpp").to_str().ok_or("path")?,
            "--root",
            project.to_str().ok_or("path")?,
            "--format",
            "json",
        ])
        .env("HOME", d.path().join("home"))
        .env("PATH", "")
        .env("TMPDIR", d.path())
        .output()?;
    assert_eq!(output.status.code(), Some(0));
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(data["complete"], true);
    assert!(
        data["coverage"]["native_incomplete_files"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
    let progress = String::from_utf8(output.stderr)?;
    assert!(
        progress.contains("Clang context incomplete") && progress.contains("not-installed.h"),
        "{progress}"
    );
    assert!(!progress.contains("time.busy="));
    assert!(!progress.contains("Capturing source changes"));
    assert!(!progress.contains("Parsing and analyzing"));
    d.close()?;
    Ok(())
}
