// Purpose: TSV 데이터 행과 metadata·완료 레코드를 구분하여 출력.
// Pattern: Output Adapter.
// Usage: CLI Renderer가 TSV 형식을 선택했을 때 호출.
// Related: crates/kodstar-cli/src/output.rs.

use kodstar_core::{QueryMetadata, QueryRow, QuerySummary};
use serde::Serialize;
use std::io::{self, Write};

fn record(writer: &mut impl Write, fields: &[&str]) -> io::Result<()> {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            writer.write_all(b"\t")?;
        }
        for character in field.chars() {
            match character {
                '\\' => writer.write_all(b"\\\\")?,
                '\t' => writer.write_all(b"\\t")?,
                '\r' => writer.write_all(b"\\r")?,
                '\n' => writer.write_all(b"\\n")?,
                _ => write!(writer, "{character}")?,
            }
        }
    }
    writer.write_all(b"\n")
}

fn detail(value: &impl Serialize) -> io::Result<String> {
    serde_json::to_string(value).map_err(io::Error::other)
}

pub(super) fn begin(writer: &mut impl Write, metadata: &QueryMetadata) -> io::Result<()> {
    writer.write_all(b"record\tkind\tdirection\trelation\tresolution\tsource_file\tsource_name\ttarget_file\ttarget_name\tuse_file\tline\tcolumn\tdetail\n")?;
    record(
        writer,
        &[
            "metadata",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            &detail(metadata)?,
        ],
    )
}

pub(super) fn row(writer: &mut impl Write, row: &QueryRow) -> io::Result<()> {
    match row {
        QueryRow::Symbol(symbol) => record(
            writer,
            &[
                "result",
                "symbol",
                "",
                "",
                "",
                &symbol.file,
                &symbol.qualified_name,
                "",
                "",
                "",
                &symbol.definition.line.to_string(),
                &symbol.definition.column.to_string(),
                &detail(row)?,
            ],
        ),
        QueryRow::Relationship(edge) => record(
            writer,
            &[
                "result",
                "relationship",
                edge.direction,
                &edge.relation,
                &edge.resolution,
                &edge.source.file,
                &edge.source.qualified_name,
                edge.target.as_ref().map_or("", |target| &target.file),
                edge.target
                    .as_ref()
                    .map(|target| target.qualified_name.as_str())
                    .or(edge.external.as_deref())
                    .unwrap_or(""),
                &edge.site.file,
                &edge.site.span.line.to_string(),
                &edge.site.span.column.to_string(),
                &detail(row)?,
            ],
        ),
        QueryRow::File(file) => record(
            writer,
            &[
                "result",
                "file",
                file.direction,
                &file.relation,
                "",
                "",
                "",
                &file.file,
                "",
                "",
                "",
                "",
                &detail(row)?,
            ],
        ),
        _ => record(
            writer,
            &[
                "result",
                super::row_kind(row),
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                &detail(row)?,
            ],
        ),
    }
}

pub(super) fn finish(writer: &mut impl Write, summary: &QuerySummary) -> io::Result<()> {
    record(
        writer,
        &[
            "summary",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            &detail(summary)?,
        ],
    )
}
