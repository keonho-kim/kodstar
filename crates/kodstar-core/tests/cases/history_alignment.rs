// Purpose: 정밀 diff의 원본 행·정렬 예산·LF 경계 보존 검증.
// Pattern: Integration Test.
// Usage: engine 테스트가 저장된 버전의 실제 diff 결과를 확인.
// Related: crates/kodstar-core/src/application/history/text/alignment.rs, crates/kodstar-core/tests/cases/history_streaming.rs.

use crate::history_streaming::difference;
use crate::support::TestResult;
use kodstar_core::HistoryRow;

#[test]
fn separated_edits_exclude_unchanged_middle_lines_and_keep_each_sides_numbers() -> TestResult {
    let rows = difference(
        b"head\nold\nsame\ntail\nremoved\nend\n",
        b"head\nnew\ninserted\nsame\ntail\nend\n",
    )?;
    let actual: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            HistoryRow::DiffLine {
                side, line, text, ..
            } => Some((side.as_str(), *line, text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            ("before", 2, "old\n"),
            ("before", 5, "removed\n"),
            ("after", 2, "new\n"),
            ("after", 3, "inserted\n"),
        ]
    );
    Ok(())
}

#[test]
fn a_window_over_the_alignment_line_budget_keeps_streaming_complete_ranges() -> TestResult {
    let before = "same\n".repeat(2200);
    let after = format!("first\n{}last\n", "same\n".repeat(2198));
    let rows = difference(before.as_bytes(), after.as_bytes())?;
    for (side, original) in [("before", before.as_str()), ("after", after.as_str())] {
        let mut restored = String::new();
        let mut expected = 1;
        for row in &rows {
            if let HistoryRow::DiffLine {
                side: found,
                line,
                text,
                ..
            } = row
                && found == side
            {
                assert_eq!(*line, expected);
                restored.push_str(text);
                expected += 1;
            }
        }
        assert_eq!(restored, original);
        assert_eq!(expected, 2201);
    }
    Ok(())
}

#[test]
fn bare_carriage_returns_remain_inside_lf_delimited_original_lines() -> TestResult {
    let rows = difference(
        b"head\rother\nold\rrow\nunchanged\rinside\nlast\nend\r",
        b"head\rother\nnew\rrow\nunchanged\rinside\nchanged\nend\r",
    )?;
    let actual: Vec<_> = rows
        .iter()
        .filter_map(|row| match row {
            HistoryRow::DiffLine {
                side, line, text, ..
            } => Some((side.as_str(), *line, text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            ("before", 2, "old\rrow\n"),
            ("before", 4, "last\n"),
            ("after", 2, "new\rrow\n"),
            ("after", 4, "changed\n"),
        ]
    );
    Ok(())
}

#[test]
fn many_carriage_returns_do_not_expand_a_single_line_or_the_alignment_budget() -> TestResult {
    let before = format!("old{}tail", "\rpart".repeat(5000));
    let after = format!("new{}tail", "\rpart".repeat(5000));
    let rows = difference(before.as_bytes(), after.as_bytes())?;
    let actual: Vec<_> = rows
        .iter()
        .filter_map(|row| match row {
            HistoryRow::DiffLine {
                side, line, text, ..
            } => Some((side.as_str(), *line, text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        actual,
        vec![("before", 1, before.as_str()), ("after", 1, after.as_str())]
    );
    Ok(())
}
