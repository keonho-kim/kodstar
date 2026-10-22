// Purpose: 격리한 HOME에서 설치·갱신·확인·완전 제거 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test installation.
// Related: crates/kodstar-cli/tests/support/mod.rs, install.sh.

mod support;
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
use support::*;

fn install(project: &Project) -> TestResult {
    let output = project.run(&["__install"])?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn initial_confirmation_requires_consent_and_root_is_always_rejected() -> TestResult {
    let project = Project::new()?;
    project.write("fn target() {}")?;
    let denied = project.command().arg("init").output()?;
    assert_eq!(denied.status.code(), Some(2));
    assert!(String::from_utf8(denied.stderr)?.contains("--yes"));
    assert!(!project.root.join(".kodstar").exists());
    assert!(!project.home.join(".kodstar").exists());
    let root = project.run(&["init", "/"])?;
    assert_eq!(root.status.code(), Some(2));
    assert!(String::from_utf8(root.stderr)?.contains("filesystem root"));
    assert!(!project.home.join(".kodstar").exists());
    assert!(project.run(&["init"])?.status.success());
    assert!(project.command().arg("init").output()?.status.success());
    Ok(())
}

#[test]
fn installing_twice_updates_in_place_and_uninstall_removes_only_owned_traces() -> TestResult {
    let project = Project::new()?;
    project.write("fn target() {}")?;
    let original = "# keep user configuration\nexport MY_VALUE=preserved";
    fs::write(project.home.join(".zshrc"), original)?;
    #[cfg(unix)]
    {
        let target = project.home.join("user-zsh-config");
        fs::rename(project.home.join(".zshrc"), &target)?;
        std::os::unix::fs::symlink(&target, project.home.join(".zshrc"))?;
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../install.sh");
    let output = Command::new("sh")
        .arg(script)
        .args(["--binary", env!("CARGO_BIN_EXE_kodstar")])
        .env("HOME", &project.home)
        .env("ZDOTDIR", &project.home)
        .env("SHELL", "/bin/zsh")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = project.home.join(".local/bin/kodstar");
    let before = fs::read(project.home.join(".zshrc"))?;
    install(&project)?;
    assert_eq!(fs::read(project.home.join(".zshrc"))?, before);
    let lookup = Command::new("/bin/sh")
        .args([
            "-c",
            ". \"$HOME/.profile\"; command -v kodstar; kodstar --version",
        ])
        .env("HOME", &project.home)
        .env("PATH", "/usr/bin:/bin")
        .output()?;
    assert!(lookup.status.success());
    assert!(
        String::from_utf8(lookup.stdout)?
            .starts_with(&fs::canonicalize(&binary)?.display().to_string())
    );
    assert!(project.run(&["init"])?.status.success());
    assert!(project.run(&["cache", "clean", "--all"])?.status.success());
    assert!(project.home.join(".kodstar/projects.sqlite").exists());
    fs::write(
        project.home.join(".kodstar/config.toml"),
        "# global settings",
    )?;
    let cancelled = project.run(&["uninstall"])?;
    assert_eq!(cancelled.status.code(), Some(2));
    assert!(binary.exists());
    let output = Command::new(&binary)
        .args(["uninstall", "--yes"])
        .env("HOME", &project.home)
        .env("ZDOTDIR", &project.home)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!binary.exists());
    assert!(!project.home.join(".kodstar").exists());
    assert!(!project.home.join(".local").exists());
    assert!(!project.root.join(".kodstar").exists());
    assert_eq!(
        fs::read_to_string(project.root.join("main.rs"))?,
        "fn target() {}"
    );
    assert_eq!(fs::read_to_string(project.home.join(".zshrc"))?, original);
    #[cfg(unix)]
    assert!(
        fs::symlink_metadata(project.home.join(".zshrc"))?
            .file_type()
            .is_symlink()
    );
    for name in [".profile", ".bashrc", ".bash_profile"] {
        assert!(!project.home.join(name).exists());
    }
    Ok(())
}

#[test]
fn unknown_project_data_and_edited_path_blocks_prevent_destructive_cleanup() -> TestResult {
    let project = Project::new()?;
    project.write("fn target() {}")?;
    install(&project)?;
    assert!(project.run(&["init"])?.status.success());
    let unknown = project.root.join(".kodstar/notes.txt");
    fs::write(&unknown, "user data")?;
    let output = project.run(&["uninstall", "--yes"])?;
    assert!(!output.status.success());
    assert!(project.root.join(".kodstar/repository.sqlite").exists());
    assert!(project.home.join(".local/bin/kodstar").exists());
    fs::remove_file(unknown)?;
    let shell = project.home.join(".zshrc");
    let text = fs::read_to_string(&shell)?;
    fs::write(
        &shell,
        text.replace("export PATH=", "export MODIFIED_PATH="),
    )?;
    let output = project.run(&["uninstall", "--yes"])?;
    assert!(!output.status.success());
    assert!(project.root.join(".kodstar/repository.sqlite").exists());
    fs::write(shell, text)?;
    assert!(project.run(&["uninstall", "--yes"])?.status.success());
    Ok(())
}

#[test]
fn active_session_blocks_uninstall_and_user_shell_additions_survive() -> TestResult {
    let project = Project::new()?;
    project.write("fn target() {}")?;
    install(&project)?;
    assert!(project.run(&["init"])?.status.success());
    let mut session = project
        .command()
        .args(["serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut input = session.stdin.take().ok_or("session stdin")?;
    input.write_all(b"{\"protocol_version\":2,\"id\":\"ping\",\"method\":\"ping\"}\n")?;
    let mut out = std::io::BufReader::new(session.stdout.take().ok_or("session stdout")?);
    let mut line = String::new();
    std::io::BufRead::read_line(&mut out, &mut line)?;
    let output = project.run(&["uninstall", "--yes"])?;
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)?.contains("in use"));
    drop(input);
    assert!(session.wait()?.success());
    let shell = project.home.join(".zshrc");
    fs::OpenOptions::new()
        .append(true)
        .open(&shell)?
        .write_all(b"# later user edit\n")?;
    assert!(project.run(&["uninstall", "--yes"])?.status.success());
    assert_eq!(fs::read_to_string(shell)?, "# later user edit\n");
    Ok(())
}
