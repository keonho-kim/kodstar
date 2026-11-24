// Purpose: 출력 sink의 수명·형식 선택과 프로세스 오류 표시.
// Pattern: Output Adapter.
// Usage: CLI 실행 경계가 엔진의 결과와 진행을 전달.
// Related: crates/kodstar-cli/src/run.rs, crates/kodstar-cli/src/output/json.rs, crates/kodstar-cli/src/output/text.rs, crates/kodstar-cli/src/output/tsv.rs.

mod history;
mod json;
mod licenses;
pub(crate) use licenses::render as render_licenses;
mod overview;
pub(crate) mod progress;
mod text;
mod tsv;
use crate::invocation::Format;
use kodstar_core::{Error, Progress, QueryMetadata, QueryRow, QuerySink, QuerySummary, Result};
use progress::ProgressPrinter;
use std::error::Error as StdError;
use std::io::{self, Write};
use std::process::ExitCode;

pub(crate) struct Renderer<W> {
    writer: W,
    format: Format,
    begun: bool,
    finished: bool,
    first: bool,
    progress: ProgressPrinter,
}

impl<W: Write> Renderer<W> {
    pub(crate) fn new(writer: W, format: Format, verbose: bool) -> Self {
        Self {
            writer,
            format,
            begun: false,
            finished: false,
            first: true,
            progress: ProgressPrinter::new(verbose),
        }
    }
}

impl<W: Write> QuerySink for Renderer<W> {
    fn begin(&mut self, metadata: &QueryMetadata) -> Result<()> {
        if self.begun {
            return Err(Error::new(
                kodstar_core::ErrorKind::Operation,
                "output metadata was already written",
            ));
        }
        match self.format {
            Format::Text => text::begin(&mut self.writer, metadata),
            Format::Tsv => tsv::begin(&mut self.writer, metadata),
            Format::Json | Format::Jsonl => json::begin(
                &mut self.writer,
                metadata,
                matches!(self.format, Format::Jsonl),
            ),
        }
        .map_err(write_error)?;
        self.begun = true;
        Ok(())
    }

    fn row(&mut self, row: &QueryRow) -> Result<()> {
        if !self.begun || self.finished {
            return Err(Error::new(
                kodstar_core::ErrorKind::Operation,
                "output row arrived outside an active result",
            ));
        }
        match self.format {
            Format::Text => text::row(&mut self.writer, row),
            Format::Tsv => tsv::row(&mut self.writer, row),
            Format::Json | Format::Jsonl => json::row(
                &mut self.writer,
                row,
                matches!(self.format, Format::Jsonl),
                self.first,
            ),
        }
        .map_err(write_error)?;
        self.first = false;
        if matches!(self.format, Format::Jsonl) {
            self.writer.flush().map_err(write_error)?;
        }
        Ok(())
    }

    fn finish(&mut self, summary: &QuerySummary) -> Result<()> {
        if !self.begun || self.finished {
            return Err(Error::new(
                kodstar_core::ErrorKind::Operation,
                "output summary arrived outside an active result",
            ));
        }
        match self.format {
            Format::Text => text::finish(&mut self.writer, summary),
            Format::Tsv => tsv::finish(&mut self.writer, summary),
            Format::Json | Format::Jsonl => json::finish(
                &mut self.writer,
                summary,
                matches!(self.format, Format::Jsonl),
            ),
        }
        .map_err(write_error)?;
        self.writer.flush().map_err(write_error)?;
        self.finished = true;
        Ok(())
    }

    fn progress(&mut self, progress: &Progress) -> Result<()> {
        self.progress.update(progress)
    }
}

fn write_error(error: io::Error) -> Error {
    Error::caused_by("write output", error)
}

fn row_kind(row: &QueryRow) -> &'static str {
    match row {
        QueryRow::History(_) => "history",
        QueryRow::Init(_) => "init",
        QueryRow::Symbol(_) => "symbol",
        QueryRow::Relationship(_) => "relationship",
        QueryRow::File(_) => "file",
        QueryRow::Documentation(_) => "documentation_reference",
        QueryRow::Overview(_) => "overview",
        QueryRow::Language(_) => "language",
        QueryRow::Status(_) => "status",
        QueryRow::Cache(_) => "cache",
        QueryRow::Diagnostic(_) => "diagnostic",
    }
}

pub(crate) fn render_argument_result(error: clap::Error) -> ExitCode {
    let exit = if error.use_stderr() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    };
    match error.print() {
        Ok(()) => exit,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "kodstar: unable to write output: {error}"
            );
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn render_error(error: &Error) -> ExitCode {
    let mut source: Option<&(dyn StdError + 'static)> = Some(error);
    while let Some(current) = source {
        if current
            .downcast_ref::<io::Error>()
            .is_some_and(|io| io.kind() == io::ErrorKind::BrokenPipe)
        {
            return ExitCode::SUCCESS;
        }
        source = current.source();
    }
    let _ = writeln!(io::stderr().lock(), "kodstar: {error}");
    ExitCode::from(error.kind.exit_code())
}
