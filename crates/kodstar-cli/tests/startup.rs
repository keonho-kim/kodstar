// Purpose: 실제 실행 파일의 시작·도움말 완전성과 부수 효과·버전 계약 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test startup.
// Related: crates/kodstar-cli/src/main.rs, crates/kodstar-cli/tests/support/mod.rs.

mod support;
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
    &["overview"],
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
            if flag == "--help" {
                let snapshot = match *command {
                    [] => Some("help_root"),
                    ["init"] => Some("help_init"),
                    ["related"] => Some("help_related"),
                    _ => None,
                };
                if let Some(name) = snapshot {
                    insta::assert_snapshot!(name, stdout);
                }
            }
            if command.is_empty() {
                assert!(stdout.contains("--version"), "{args:?}");
            }
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
fn version_reports_cli_and_engine_versions() -> TestResult {
    let project = Project::new()?;
    for flag in ["--version", "-V"] {
        let output = project.run(&[flag])?;

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
fn bundled_license_notices_are_available_without_initializing_a_project() -> TestResult {
    let project = Project::new()?;
    let cases: &[&[&str]] = &[&["--licenses"], &["--verbose", "false", "--licenses"]];
    for arguments in cases {
        let output = project.run(arguments)?;
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout)?;
        assert!(text.contains("LLVM 23.1.4") && text.contains("html-escape 0.3.0"));
        let mut expected = b"Third-party notices\n\n".to_vec();
        expected.extend_from_slice(include_bytes!("../../../LICENSES/third-party-notices.txt"));
        expected.extend(zstd::stream::decode_all(
            kodstar_clang::HEADER_NOTICES_ZSTD,
        )?);
        assert_eq!(text.as_bytes(), expected);
    }
    assert!(!project.root.join(".kodstar").exists());
    Ok(())
}

#[test]
fn invalid_or_missing_arguments_fail_on_stderr() -> TestResult {
    let project = Project::new()?;
    let cases: &[&[&str]] = &[
        &["--unknown"],
        &["unknown-command"],
        &[],
        &["--licenses", "init"],
    ];
    for arguments in cases {
        let output = project.run(arguments)?;
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert!(!output.stderr.is_empty(), "{arguments:?}");
        if arguments.is_empty() {
            assert!(String::from_utf8(output.stderr)?.contains("Usage: kodstar"));
        }
    }
    Ok(())
}
