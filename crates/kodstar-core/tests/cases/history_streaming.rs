// Purpose: 청크 경계·UTF-8·긴 행·빈 입력의 diff 출력 보존 검증.
// Pattern: Integration Test.
// Usage: engine 테스트가 저장된 두 버전의 실제 행을 비교.
// Related: crates/kodstar-core/src/application/history/text.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

fn difference(
    before: &[u8],
    after: &[u8],
) -> std::result::Result<Vec<HistoryRow>, Box<dyn std::error::Error>> {
    let p = Project::new()?;
    std::fs::write(p.root.join("text.txt"), before)?;
    p.build(false)?;
    std::fs::write(p.root.join("text.txt"), after)?;
    p.history(HistoryRequest::Save {
        name: "changed".into(),
    })?;
    let diff = p.history(HistoryRequest::Diff {
        from: Some("c1".into()),
        to: Some("c2".into()),
        refresh: Refresh::None,
    })?;
    Ok(diff
        .rows
        .into_iter()
        .filter_map(|r| {
            if let QueryRow::History(row) = r {
                Some(row)
            } else {
                None
            }
        })
        .collect())
}

#[test]
fn unicode_chunk_boundaries_and_crlf_keep_original_line_numbers() -> TestResult {
    let prefix = format!("{}🦀\r\n", "x".repeat(65534));
    let tail = "unchanged\n".repeat(15000);
    let before = format!("{prefix}old 한글\r\n{tail}");
    let after = format!("{prefix}new 한글\r\n{tail}");
    let rows = difference(before.as_bytes(), after.as_bytes())?;
    let lines: Vec<_> = rows
        .iter()
        .filter_map(|r| {
            if let HistoryRow::DiffLine {
                side, line, text, ..
            } = r
            {
                Some((side.as_str(), *line, text.as_str()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        lines,
        vec![("before", 2, "old 한글\r\n"), ("after", 2, "new 한글\r\n")]
    );
    Ok(())
}

#[test]
fn long_changed_lines_are_complete_without_a_trailing_newline() -> TestResult {
    let before = format!("head\n{}old{}", "a".repeat(80000), "🦀".repeat(20000));
    let after = before.replace("old", "new");
    let rows = difference(before.as_bytes(), after.as_bytes())?;
    let lines: Vec<_> = rows
        .iter()
        .filter_map(|r| {
            if let HistoryRow::DiffLine {
                side, line, text, ..
            } = r
            {
                Some((side.as_str(), *line, text.as_str()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        lines,
        vec![("before", 2, &before[5..]), ("after", 2, &after[5..])]
    );
    Ok(())
}

#[test]
fn empty_inputs_and_changes_at_a_complete_line_boundary_remain_exact() -> TestResult {
    for (before, after, side, line, text) in [
        ("", "한글", "after", 1, "한글"),
        ("removed\n", "", "before", 1, "removed\n"),
        ("same\n", "same\nadded\n", "after", 2, "added\n"),
        ("same\nremoved\n", "same\n", "before", 2, "removed\n"),
    ] {
        let rows = difference(before.as_bytes(), after.as_bytes())?;
        let lines: Vec<_> = rows
            .iter()
            .filter_map(|r| {
                if let HistoryRow::DiffLine {
                    side, line, text, ..
                } = r
                {
                    Some((side.as_str(), *line, text.as_str()))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(lines, vec![(side, line, text)]);
    }
    Ok(())
}

#[test]
fn incomplete_utf8_is_omitted_after_complete_content_verification() -> TestResult {
    let mut after = vec![b'x'; 65535];
    after.extend_from_slice(&[0xf0, 0x9f]);
    let rows = difference(b"valid\n", &after)?;
    assert!(rows.iter().any(|r|matches!(r, HistoryRow::DiffOmitted {side,reason,..} if side=="after" && reason=="binary_content")));
    assert!(
        !rows
            .iter()
            .any(|r| matches!(r, HistoryRow::DiffLine {side,..} if side=="after"))
    );
    Ok(())
}

#[test]
fn corrupted_line_metadata_cannot_change_the_reported_diff_range() -> TestResult {
    let p = Project::new()?;
    p.write("text.txt", "same\nold\ntail\n")?;
    p.build(false)?;
    p.write("text.txt", "same\nnew\ntail\n")?;
    p.history(HistoryRequest::Save {
        name: "modified".into(),
    })?;
    let connection = rusqlite::Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    connection.execute(
        "UPDATE chunk_data SET lines=0,first_newline=-1,last_newline=-1",
        [],
    )?;
    drop(connection);
    let error = p
        .history(HistoryRequest::Diff {
            from: Some("c1".into()),
            to: Some("c2".into()),
            refresh: Refresh::None,
        })
        .expect_err("unverified line metadata was used");
    assert!(error.to_string().contains("checksum mismatch"), "{error}");
    Ok(())
}

#[test]
fn inserting_a_full_chunk_preserves_suffix_alignment_and_original_context() -> TestResult {
    let tail = "tail line\n".repeat(15000);
    let insertion = format!("{}\n", "z".repeat(65535));
    let before = format!("head\n{tail}");
    let after = format!("head\n{insertion}{tail}");
    let rows = difference(before.as_bytes(), after.as_bytes())?;
    let lines: Vec<_> = rows
        .iter()
        .filter_map(|r| {
            if let HistoryRow::DiffLine {
                side, line, text, ..
            } = r
            {
                Some((side.as_str(), *line, text.as_str()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        lines,
        vec![
            ("before", 2, "tail line\n"),
            ("after", 2, insertion.as_str()),
            ("after", 3, "tail line\n")
        ]
    );
    Ok(())
}
