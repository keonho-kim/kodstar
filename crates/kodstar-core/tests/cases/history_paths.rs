// Purpose: 내부 경로·symlink·파일 구조 변화와 복원 경계 검증.
// Pattern: Integration Test.
// Usage: engine 회귀 테스트가 임시 파일시스템에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;
use std::fs;

#[test]
fn native_history_ignores_git_worktree_control_files() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn local() {}")?;
    p.write(".git", "gitdir: ../missing-git-metadata\n")?;
    let report = p.build(false)?;
    assert_eq!(report.history.ok_or("history")?.tracked_files, 1);
    Ok(())
}

#[test]
fn branch_switch_handles_a_file_replaced_by_a_directory() -> TestResult {
    let p = Project::new()?;
    p.write("entry", "original file")?;
    p.build(false)?;
    p.history(HistoryRequest::CreateBranch {
        name: "file".into(),
    })?;
    fs::remove_file(p.root.join("entry"))?;
    p.write("entry/nested.rs", "fn nested() {}")?;
    p.build(false)?;
    p.history(HistoryRequest::SwitchBranch {
        name: "file".into(),
    })?;
    assert_eq!(fs::read_to_string(p.root.join("entry"))?, "original file");
    p.history(HistoryRequest::SwitchBranch {
        name: "main".into(),
    })?;
    assert_eq!(
        fs::read_to_string(p.root.join("entry/nested.rs"))?,
        "fn nested() {}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn selected_symlink_restore_preserves_outside_targets() -> TestResult {
    let p = Project::new()?;
    let first = p.directory.join("first");
    let second = p.directory.join("second");
    fs::write(&first, "first outside content")?;
    fs::write(&second, "second outside content")?;
    std::os::unix::fs::symlink(&first, p.root.join("link"))?;
    p.build(false)?;
    fs::remove_file(p.root.join("link"))?;
    std::os::unix::fs::symlink(&second, p.root.join("link"))?;
    p.history(HistoryRequest::Restore {
        revision: "c1".into(),
        path: Some("link".into()),
    })?;
    assert_eq!(fs::read_link(p.root.join("link"))?, first);
    assert_eq!(fs::read_to_string(&second)?, "second outside content");
    Ok(())
}
