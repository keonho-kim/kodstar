// Purpose: CLI 메타데이터 명령의 동기화·명시적 cached 읽기·복구 예외 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test synchronization.
// Related: crates/kodstar-cli/tests/support/mod.rs, crates/kodstar-cli/src/run.rs.

mod support;
use serde_json::Value;
use support::*;

#[test]
fn metadata_commands_share_synchronization_and_keep_explicit_recovery_paths() -> TestResult {
    let project = Project::new()?;
    project.write("fn first() {}")?;
    assert!(project.run(&["init"])?.status.success());
    for (index, args) in [
        vec!["status"],
        vec!["doctor"],
        vec!["languages"],
        vec!["cache", "list"],
    ]
    .into_iter()
    .enumerate()
    {
        project.write(&format!("fn version_{index}() {{}}"))?;
        let mut args = args;
        args.extend(["--format", "json"]);
        let result = project.run(&args)?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let data: Value = serde_json::from_slice(&result.stdout)?;
        assert_eq!(data["generation"], index + 2);
        assert_eq!(data["freshness"], "metadata_checked");
    }
    project.write("fn broken( {")?;
    for args in [
        vec!["status"],
        vec!["doctor"],
        vec!["languages"],
        vec!["cache", "list"],
    ] {
        let result = project.run(&args)?;
        assert_eq!(
            result.status.code(),
            Some(if matches!(args[0], "status" | "doctor") {
                0
            } else {
                1
            })
        );
        if matches!(args[0], "status" | "doctor") {
            let diagnostic = project.run(&[args[0], "--format", "json"])?;
            let data: Value = serde_json::from_slice(&diagnostic.stdout)?;
            assert!(data["history"]["analysis_error"].is_string());
            assert_ne!(
                data["history"]["checkpoint"],
                data["history"]["analysis_checkpoint"]
            );
        }
        let mut args = args;
        args.extend(["--no-refresh", "--format", "json"]);
        let result = project.run(&args)?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let data: Value = serde_json::from_slice(&result.stdout)?;
        assert_eq!(data["freshness"], "not_checked");
    }
    assert!(project.run(&["--help"])?.status.success());
    assert!(project.run(&["--version"])?.status.success());
    assert!(project.run(&["cache", "clean"])?.status.success());
    Ok(())
}
