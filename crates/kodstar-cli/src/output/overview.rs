// Purpose: 프로젝트 개요의 집계와 근거 위치를 사람이 읽는 형식으로 표시.
// Pattern: Output Adapter.
// Usage: CLI text renderer가 Overview 행을 전달.
// Related: crates/kodstar-cli/src/output/text.rs, crates/kodstar-core/src/application/query/overview.rs.

use kodstar_core::OverviewRow;
use std::io::{self, Write};

pub(super) fn row(writer: &mut impl Write, row: &OverviewRow) -> io::Result<()> {
    match row {
        OverviewRow::Module { path, files, .. } => {
            writeln!(writer, "directory {path} | files {files}")
        }
        OverviewRow::ModuleDependency {
            from,
            to,
            relation,
            count,
            site,
            ..
        } => writeln!(
            writer,
            "{from} -> {to} | {relation} {count} | use {}:{}:{}",
            site.file, site.span.line, site.span.column
        ),
        OverviewRow::PackageDependency {
            from,
            to,
            relation,
            count,
            site,
            ..
        } => writeln!(
            writer,
            "package {} -> {} | {relation} {count} | use {}:{}:{}",
            from.path, to.path, site.file, site.span.line, site.span.column
        ),
        OverviewRow::Hub { symbol, incoming } => writeln!(
            writer,
            "{} | users {incoming} | {}:{}:{}",
            symbol.qualified_name, symbol.file, symbol.definition.line, symbol.definition.column
        ),
        OverviewRow::ImportCycle { files, sample } => {
            write!(writer, "import cycle | files {files}")?;
            for file in sample {
                write!(writer, " | {}", file.path)?;
            }
            writeln!(writer)
        }
    }
}
