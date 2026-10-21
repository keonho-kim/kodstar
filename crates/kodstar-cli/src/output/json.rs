// Purpose: JSON과 JSONL 결과의 스트리밍 framing과 완료 표시.
// Pattern: Output Adapter.
// Usage: CLI Renderer가 metadata·행·summary 순서로 호출.
// Related: crates/kodstar-cli/src/output.rs.

use kodstar_core::{QueryMetadata, QueryRow, QuerySummary};
use serde::Serialize;
use std::io::{self, Write};

#[derive(Serialize)]
struct Event<'a, T> {
    #[serde(rename = "type")]
    event: &'static str,
    #[serde(flatten)]
    value: &'a T,
}

fn encode(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(writer, value).map_err(io::Error::other)
}

pub(super) fn begin(
    writer: &mut impl Write,
    metadata: &QueryMetadata,
    lines: bool,
) -> io::Result<()> {
    if lines {
        encode(
            writer,
            &Event {
                event: "metadata",
                value: metadata,
            },
        )?;
        writer.write_all(b"\n")
    } else {
        // Only this small metadata object is buffered; result rows are never collected.
        let mut header = serde_json::to_vec(metadata).map_err(io::Error::other)?;
        if header.pop() != Some(b'}') {
            return Err(io::Error::other("metadata must serialize as an object"));
        }
        writer.write_all(&header)?;
        writer.write_all(b",\"results\":[")
    }
}

pub(super) fn row(
    writer: &mut impl Write,
    row: &QueryRow,
    lines: bool,
    first: bool,
) -> io::Result<()> {
    if lines {
        encode(
            writer,
            &Event {
                event: "result",
                value: row,
            },
        )?;
        writer.write_all(b"\n")
    } else {
        if !first {
            writer.write_all(b",")?;
        }
        encode(writer, row)
    }
}

pub(super) fn finish(
    writer: &mut impl Write,
    summary: &QuerySummary,
    lines: bool,
) -> io::Result<()> {
    if lines {
        encode(
            writer,
            &Event {
                event: "summary",
                value: summary,
            },
        )?;
        writer.write_all(b"\n")
    } else {
        write!(
            writer,
            "],\"complete\":{},\"truncated\":{},\"result_count\":{},\"visited_count\":{},\"reasons\":",
            summary.complete, summary.truncated, summary.result_count, summary.visited_count
        )?;
        encode(writer, &summary.reasons)?;
        writer.write_all(b"}\n")
    }
}
