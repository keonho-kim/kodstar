// Purpose: 정의·사용 위치와 분석 상태를 사람이 읽는 행으로 표시.
// Pattern: Output Adapter.
// Usage: CLI Renderer가 text 형식의 행과 완료를 전달.
// Related: crates/kodstar-cli/src/output.rs.

use kodstar_core::{QueryMetadata, QueryRow, QuerySummary};
use std::io::{self, Write};

pub(super) fn begin(writer: &mut impl Write, metadata: &QueryMetadata) -> io::Result<()> {
    if let Some(root) = &metadata.root {
        writeln!(
            writer,
            "project {root} | generation {} | {}",
            metadata
                .generation
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into()),
            metadata.freshness
        )?;
        if let Some(state) = &metadata.history {
            writeln!(
                writer,
                "branch {} | checkpoint {} | analysis {} | changed {} | tracked {}",
                state.branch,
                state.checkpoint,
                state.analysis_checkpoint.as_deref().unwrap_or("-"),
                state.changed_files,
                state.tracked_files
            )?;
        }
    }
    let c = &metadata.coverage;
    if c.skipped_files > 0
        || c.partial_files > 0
        || c.fragment_files > 0
        || c.configuration_incomplete_files > 0
        || c.native_incomplete_files > 0
        || c.native_limited_files > 0
    {
        writeln!(
            writer,
            "analysis coverage | skipped {} (failed {}) | syntax partial {} | fragments {} | configuration incomplete {} | native incomplete {} | native limited {}",
            c.skipped_files,
            c.failed_files,
            c.partial_files,
            c.fragment_files,
            c.configuration_incomplete_files,
            c.native_incomplete_files,
            c.native_limited_files
        )?;
    }
    Ok(())
}

pub(super) fn row(writer: &mut impl Write, row: &QueryRow) -> io::Result<()> {
    match row {
        QueryRow::History(row) => super::history::row(writer, row),
        QueryRow::Overview(row) => super::overview::row(writer, row),
        QueryRow::Documentation(reference) => {
            write!(
                writer,
                "{} {} | use {}:{}:{}",
                reference.document_kind,
                reference.reference,
                reference.source.file,
                reference.source.span.line,
                reference.source.span.column
            )?;
            if let Some(target) = &reference.target {
                writeln!(
                    writer,
                    " -> {}:{}:{}",
                    target.file, target.span.line, target.span.column
                )
            } else {
                writeln!(writer, " | {}", reference.reason.unwrap_or("unresolved"))
            }
        }
        QueryRow::Init(report) => writeln!(
            writer,
            "indexed generation {} | parsed {} | reused {} | skipped {} | {} ms",
            report.generation,
            report.parsed_files,
            report.coverage.reused_files,
            report.coverage.skipped_files,
            report.duration_ms
        ),
        QueryRow::Symbol(symbol) => writeln!(
            writer,
            "{}  {}  {}:{}:{}",
            symbol.kind,
            symbol.qualified_name,
            symbol.file,
            symbol.definition.line,
            symbol.definition.column
        ),
        QueryRow::Relationship(edge) => {
            let target = edge
                .target
                .as_ref()
                .map(|target| target.qualified_name.as_str())
                .or(edge.external.as_deref())
                .unwrap_or("<unresolved>");
            writeln!(
                writer,
                "{}  {}  {} -> {}  [{}; {}]",
                edge.direction,
                edge.relation,
                edge.source.qualified_name,
                target,
                edge.resolution,
                edge.rule
            )?;
            if let Some(target) = &edge.target {
                writeln!(
                    writer,
                    "  definition {}:{}:{}",
                    target.file, target.definition.line, target.definition.column
                )?;
            }
            writeln!(
                writer,
                "  use {}:{}:{} | count {} | depth {}",
                edge.site.file, edge.site.span.line, edge.site.span.column, edge.count, edge.depth
            )?;
            if let Some(reason) = &edge.reason {
                writeln!(writer, "  reason: {reason}")?;
            }
            Ok(())
        }
        QueryRow::File(file) => writeln!(
            writer,
            "{}  {}  {}  {}",
            file.direction, file.relation, file.file, file.count
        ),
        QueryRow::Language(language) => {
            writeln!(
                writer,
                "{}  {} | definitions={} files={} direct={} semantic={}",
                language.language,
                language.extensions.join(","),
                language.definition_extraction,
                language.file_relationships,
                language.direct_relationships,
                language.semantic_enrichment
            )?;
            for limitation in &language.limitations {
                writeln!(writer, "  limitation: {limitation}")?;
            }
            Ok(())
        }
        QueryRow::Status(status) => {
            writeln!(
                writer,
                "cache {} | last update {} | storage {}",
                status.cache, status.last_update, status.storage_version
            )?;
            writeln!(
                writer,
                "files {} | analyzed {} | skipped {} | unsupported {} | unresolved {}",
                status.coverage.scanned_files,
                status.coverage.analyzed_files,
                status.coverage.skipped_files,
                status.coverage.unsupported_files,
                status.coverage.unresolved
            )?;
            if let Some(error) = &status.last_error {
                writeln!(writer, "last error: {error}")?;
            }
            if let Some(integrity) = &status.integrity {
                writeln!(writer, "integrity: {integrity}")?;
            }
            Ok(())
        }
        QueryRow::Cache(cache) => writeln!(
            writer,
            "{}  {}  {} bytes  active={}  {}",
            cache.action,
            cache.root,
            cache.bytes,
            cache.active,
            cache.reason.unwrap_or("")
        ),
        QueryRow::Diagnostic(diagnostic) => {
            writeln!(writer, "{}  {}", diagnostic.code, diagnostic.message)
        }
    }
}

pub(super) fn finish(writer: &mut impl Write, summary: &QuerySummary) -> io::Result<()> {
    writeln!(
        writer,
        "results {} | complete={} truncated={} | unresolved {}{}",
        summary.result_count,
        summary.complete,
        summary.truncated,
        summary.unresolved_count,
        if summary.reasons.is_empty() {
            String::new()
        } else {
            format!(" | {}", summary.reasons.join(","))
        }
    )
}
