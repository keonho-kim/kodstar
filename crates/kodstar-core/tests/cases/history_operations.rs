// Purpose: 이름·복원·브랜치와 과거 분석의 관찰 가능한 계약 검증.
// Pattern: Integration Test.
// Usage: engine 회귀 테스트가 임시 저장소에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;
use std::fs;

#[test]
fn save_names_existing_state_and_restore_preserves_unsynchronized_edits() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn original() {}")?;
    p.build(false)?;
    p.history(HistoryRequest::Save {
        name: "baseline".into(),
    })?;
    let list = p.history(HistoryRequest::List {
        refresh: Refresh::None,
    })?;
    assert_eq!(list.rows.len(), 1);
    p.write("main.rs", "fn unsynchronized() {}")?;
    p.history(HistoryRequest::Restore {
        revision: "baseline".into(),
        path: None,
    })?;
    assert_eq!(
        fs::read_to_string(p.root.join("main.rs"))?,
        "fn original() {}"
    );
    let list = p.history(HistoryRequest::List {
        refresh: Refresh::None,
    })?;
    assert_eq!(list.rows.len(), 3);
    p.history(HistoryRequest::Restore {
        revision: "c2".into(),
        path: None,
    })?;
    assert_eq!(
        fs::read_to_string(p.root.join("main.rs"))?,
        "fn unsynchronized() {}"
    );
    Ok(())
}

#[test]
fn branches_diverge_and_switch_preserves_current_changes() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn start() {}")?;
    p.build(false)?;
    p.history(HistoryRequest::CreateBranch {
        name: "experiment".into(),
    })?;
    p.write("main.rs", "fn main_change() {}")?;
    p.history(HistoryRequest::SwitchBranch {
        name: "experiment".into(),
    })?;
    assert_eq!(fs::read_to_string(p.root.join("main.rs"))?, "fn start() {}");
    p.write("main.rs", "fn experiment_change() {}")?;
    p.history(HistoryRequest::SwitchBranch {
        name: "main".into(),
    })?;
    assert_eq!(
        fs::read_to_string(p.root.join("main.rs"))?,
        "fn main_change() {}"
    );
    p.history(HistoryRequest::SwitchBranch {
        name: "experiment".into(),
    })?;
    assert_eq!(
        fs::read_to_string(p.root.join("main.rs"))?,
        "fn experiment_change() {}"
    );
    assert!(
        p.history(HistoryRequest::DeleteBranch {
            name: "experiment".into()
        })
        .is_err()
    );
    Ok(())
}

#[test]
fn past_query_does_not_change_files_or_capture_current_edits() -> TestResult {
    let p = Project::new()?;
    p.write("deleted.rs", "fn past_symbol() {}")?;
    p.build(false)?;
    fs::remove_file(p.root.join("deleted.rs"))?;
    p.write("current.rs", "fn current_symbol() {}")?;
    let mut result = Collected::default();
    p.engine.query(
        &p.options(),
        &Query::Symbols {
            file: "deleted.rs".into(),
        },
        &QueryOptions {
            revision: Some("c1".into()),
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut result,
    )?;
    assert!(
        result
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="past_symbol"))
    );
    assert!(!p.root.join("deleted.rs").exists());
    let list = p.history(HistoryRequest::List {
        refresh: Refresh::None,
    })?;
    assert_eq!(list.rows.len(), 1);
    Ok(())
}

#[test]
fn partial_analysis_does_not_block_saved_history() -> TestResult {
    let p = Project::new()?;
    p.write("main.ts", "export function valid() {}")?;
    p.write("tsconfig.json", "{ broken configuration")?;
    assert_eq!(p.build(false)?.coverage.failed_files, 1);
    let list = p.history(HistoryRequest::List {
        refresh: Refresh::None,
    })?;
    assert!(
        list.rows
            .iter()
            .any(|r| matches!(r,QueryRow::History(HistoryRow::Checkpoint{id,..}) if id=="c1"))
    );
    p.history(HistoryRequest::Restore {
        revision: "c1".into(),
        path: None,
    })?;
    Ok(())
}

#[test]
fn explicit_diff_reads_saved_sources_without_capturing_edits() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn before() {}\n")?;
    p.build(false)?;
    p.write("main.rs", "fn after() {}\n")?;
    p.history(HistoryRequest::Save {
        name: "after".into(),
    })?;
    p.write("main.rs", "fn later() {}\n")?;
    let diff = p.history(HistoryRequest::Diff {
        from: Some("c1".into()),
        to: Some("after".into()),
        refresh: Refresh::None,
    })?;
    assert!(diff.rows.iter().any(|r|matches!(r,QueryRow::History(HistoryRow::DiffLine{side,text,..}) if side=="before" && text.contains("before"))));
    assert!(diff.rows.iter().any(|r|matches!(r,QueryRow::History(HistoryRow::DiffLine{side,text,..}) if side=="after" && text.contains("after"))));
    assert_eq!(
        p.history(HistoryRequest::List {
            refresh: Refresh::None
        })?
        .rows
        .len(),
        2
    );
    Ok(())
}
