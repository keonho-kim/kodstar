// Purpose: 실제 실행 파일의 시작·도움말 완전성과 부수 효과·버전 계약 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test startup.
// Related: crates/kodstar-cli/src/main.rs, crates/kodstar-cli/tests/support/mod.rs.

mod support;
use std::process::Command;
use support::{Project, TestResult};

const COMMANDS: &[&[&str]] = &[
    &[],
    &["init"],
    &["uninstall"],
    &["serve"],
    &["watch"],
    &["symbols"],
    &["list"],
    &["search"],
    &["related"],
    &["deps"],
    &["refs"],
    &["explain"],
    &["impact"],
    &["path"],
    &["status"],
    &["doctor"],
    &["languages"],
    &["cache"],
    &["cache", "list"],
    &["cache", "clean"],
    &["cache", "gc"],
    &["history"],
    &["history", "save"],
    &["history", "diff"],
    &["history", "restore"],
    &["history", "upgrade"],
    &["branch"],
    &["branch", "create"],
    &["branch", "switch"],
    &["branch", "delete"],
];

#[test]
fn every_command_has_short_and_detailed_help_without_creating_state() -> TestResult {
    let project = Project::new()?;
    let source = "fn original() {}\n";
    project.write(source)?;
    for command in COMMANDS {
        for flag in ["-h", "--help"] {
            let mut args = command.to_vec();
            args.push(flag);
            let output = project.run(&args)?;
            assert!(output.status.success(), "{args:?}");
            assert!(output.stderr.is_empty(), "{args:?}");
            let stdout = String::from_utf8(output.stdout)?;
            assert!(stdout.contains("Usage: kodstar"), "{args:?}");
            assert!(!stdout.starts_with("Usage:"), "missing summary: {args:?}");
            assert_eq!(stdout.contains("Examples:"), flag == "--help", "{args:?}");
        }
    }
    assert!(std::fs::read_dir(&project.home)?.next().is_none());
    assert_eq!(std::fs::read_dir(&project.root)?.count(), 1);
    assert_eq!(
        std::fs::read_to_string(project.root.join("main.rs"))?,
        source
    );
    Ok(())
}

#[test]
fn help_succeeds_on_stdout() -> Result<(), Box<dyn std::error::Error>> {
    for flag in ["--help", "-h"] {
        let output = Command::new(env!("CARGO_BIN_EXE_kodstar"))
            .arg(flag)
            .output()?;

        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8(output.stdout)?;
        assert!(stdout.contains("Usage: kodstar"));
        assert!(stdout.contains("--version"));
    }
    Ok(())
}

#[test]
fn version_reports_cli_and_engine_versions() -> Result<(), Box<dyn std::error::Error>> {
    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_kodstar"))
            .arg(flag)
            .output()?;

        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(
            String::from_utf8(output.stdout)?.trim(),
            format!(
                "kodstar {} (engine {})",
                env!("CARGO_PKG_VERSION"),
                kodstar_core::VERSION
            )
        );
    }
    Ok(())
}

#[test]
fn invalid_arguments_fail_on_stderr() -> Result<(), Box<dyn std::error::Error>> {
    for argument in ["--unknown", "unknown-command"] {
        let output = Command::new(env!("CARGO_BIN_EXE_kodstar"))
            .arg(argument)
            .output()?;

        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn missing_command_displays_usage_and_fails() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_kodstar")).output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)?.contains("Usage: kodstar"));
    Ok(())
}
