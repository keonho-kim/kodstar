// Purpose: 실제 실행 파일의 시작·도움말·버전 계약 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test startup.
// Related: crates/kodstar-cli/src/main.rs.

use std::process::Command;

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
