// Purpose: 마지막 정상 분석과 체크포인트 순변화의 입력 선택·실패·분기 일관성 검증.
// Pattern: Differential Integration Test.
// Usage: engine이 기본 증분 갱신과 강제 전체 분석 결과를 대조.
// Related: crates/kodstar-core/src/application/analysis/incremental/inputs.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;

fn fixture() -> std::result::Result<Project, Box<dyn std::error::Error>> {
    let p = Project::new()?;
    p.write("go.mod", "module example.test/api\ngo 1.23\n")?;
    for i in 0..8 {
        p.write(
            &format!("file{i}.go"),
            &format!("package api\nfunc F{i}() {{}}\n"),
        )?;
    }
    p.build(false)?;
    Ok(p)
}

fn refresh(p: &Project) -> kodstar_core::Result<InitReport> {
    p.engine.init(
        &p.options(),
        InitOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )
}

fn matches_full(p: &Project, report: &InitReport) -> TestResult {
    assert_eq!(
        normalized_index(report)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn net_reverts_skip_extraction_from_the_last_analyzed_checkpoint() -> TestResult {
    let p = fixture()?;
    p.write("file0.go", "package api\nfunc Intermediate() {}\n")?;
    p.history(HistoryRequest::Save {
        name: "intermediate".into(),
    })?;
    p.write("file0.go", "package api\nfunc F0() {}\n")?;
    p.history(HistoryRequest::Save {
        name: "reverted".into(),
    })?;
    let report = refresh(&p)?;
    assert_eq!(report.parsed_files, 0);
    assert_eq!(report.coverage.reused_files, 9);
    assert_eq!(report.history.as_ref().ok_or("history")?.checkpoint, "c3");
    matches_full(&p, &report)
}

#[test]
fn partial_analysis_and_unanalysed_checkpoints_keep_a_consistent_delta_baseline() -> TestResult {
    let p = fixture()?;
    p.write("file0.go", "package api\nfunc broken( {\n")?;
    assert_eq!(refresh(&p)?.coverage.partial_files, 1);
    p.write("file1.go", "package api\nfunc F1() { Added() }\n")?;
    p.history(HistoryRequest::Save {
        name: "unanalysed".into(),
    })?;
    p.write("file0.go", "package api\nfunc Added() {}\n")?;
    let report = refresh(&p)?;
    assert_eq!(report.parsed_files, 2);
    assert_eq!(report.coverage.reused_files, 7);
    matches_full(&p, &report)
}

#[test]
fn branch_delta_additions_deletions_and_renames_match_full_analysis() -> TestResult {
    let p = fixture()?;
    p.history(HistoryRequest::CreateBranch {
        name: "alternate".into(),
    })?;
    p.write("file0.go", "package api\nfunc Main() {}\n")?;
    refresh(&p)?;
    p.history(HistoryRequest::SwitchBranch {
        name: "alternate".into(),
    })?;
    std::fs::remove_file(p.root.join("file1.go"))?;
    std::fs::rename(p.root.join("file2.go"), p.root.join("moved.go"))?;
    p.write("new.go", "package api\nfunc New() { F0() }\n")?;
    let report = refresh(&p)?;
    matches_full(&p, &report)?;
    p.history(HistoryRequest::SwitchBranch {
        name: "main".into(),
    })?;
    let report = refresh(&p)?;
    matches_full(&p, &report)
}

#[test]
fn new_package_markers_reclassify_unchanged_descendants() -> TestResult {
    let p = fixture()?;
    p.write("member/main.go", "package member\nfunc Member() {}\n")?;
    refresh(&p)?;
    p.write("member/go.mod", "module example.test/member\ngo 1.23\n")?;
    let report = refresh(&p)?;
    assert!(report.parsed_files >= 2);
    matches_full(&p, &report)?;
    std::fs::remove_file(p.root.join("member/go.mod"))?;
    let report = refresh(&p)?;
    matches_full(&p, &report)
}

#[cfg(unix)]
#[test]
fn regular_to_symlink_and_back_updates_the_exact_analysis_inventory() -> TestResult {
    let p = fixture()?;
    std::fs::remove_file(p.root.join("file0.go"))?;
    std::os::unix::fs::symlink("file1.go", p.root.join("file0.go"))?;
    let report = refresh(&p)?;
    assert_eq!(report.coverage.scanned_files, 8);
    matches_full(&p, &report)?;
    std::fs::remove_file(p.root.join("file0.go"))?;
    p.write("file0.go", "package api\nfunc Restored() {}\n")?;
    let report = refresh(&p)?;
    assert_eq!(report.coverage.scanned_files, 9);
    matches_full(&p, &report)
}
