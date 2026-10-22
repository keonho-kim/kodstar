// Purpose: 이력·브랜치·파일 diff의 사람이 읽는 표시.
// Pattern: Output Adapter.
// Usage: text renderer가 구조화된 HistoryRow를 전달.
// Related: crates/kodstar-cli/src/output/text.rs.

use kodstar_core::HistoryRow;
use std::io::{self, Write};

pub(super) fn row(writer: &mut impl Write, row: &HistoryRow) -> io::Result<()> {
    match row {
        HistoryRow::Impact {
            checkpoint,
            relationship,
        } => writeln!(
            writer,
            "{checkpoint}  {}  {} -> {}",
            relationship.relation,
            relationship.source.qualified_name,
            relationship
                .target
                .as_ref()
                .map(|s| s.qualified_name.as_str())
                .unwrap_or("<unresolved>")
        ),
        HistoryRow::Checkpoint {
            id,
            parent,
            created_at,
            name,
            changed_files,
        } => writeln!(
            writer,
            "{id}  parent={}  time={created_at}  changed={changed_files}{}",
            parent.as_deref().unwrap_or("-"),
            name.as_ref()
                .map(|s| format!("  name={s}"))
                .unwrap_or_default()
        ),
        HistoryRow::Branch {
            name,
            checkpoint,
            current,
        } => writeln!(
            writer,
            "{} {name}  {checkpoint}",
            if *current { "*" } else { " " }
        ),
        HistoryRow::Change {
            path,
            change,
            before,
            after,
            ..
        } => writeln!(
            writer,
            "{change}  {path}  {} -> {}",
            before.as_deref().unwrap_or("-"),
            after.as_deref().unwrap_or("-")
        ),
        HistoryRow::DiffLine {
            path,
            side,
            line,
            text,
            ..
        } => write!(
            writer,
            "{} {path}:{line} {text}{}",
            if side == "before" { "-" } else { "+" },
            if text.ends_with('\n') { "" } else { "\n" }
        ),
        HistoryRow::DiffOmitted {
            path, side, reason, ..
        } => writeln!(
            writer,
            "{path} [{side}: {reason}; content hashes shown above]"
        ),
        HistoryRow::Operation {
            operation,
            checkpoint,
            branch,
            changed_files,
        } => writeln!(
            writer,
            "{operation}  {branch}  {checkpoint}  changed={changed_files}"
        ),
    }
}
