// Purpose: 대량 파일 처리의 진행을 stderr에 제한적으로 표시.
// Pattern: Output Adapter.
// Usage: build callback과 CLI 결과 sink가 진행 이벤트를 전달.
// Related: crates/kodstar-cli/src/output.rs, crates/kodstar-cli/src/run.rs.

use kodstar_core::{Error, Progress, Result};
use std::io::{self, IsTerminal, Write};

pub(crate) struct ProgressPrinter {
    stderr: io::Stderr,
    phase: Option<&'static str>,
    last_files: u64,
}

impl ProgressPrinter {
    pub(crate) fn new() -> Self {
        Self {
            stderr: io::stderr(),
            phase: None,
            last_files: 0,
        }
    }

    pub(crate) fn update(&mut self, progress: &Progress) -> Result<()> {
        if !self.stderr.is_terminal() {
            return Ok(());
        }
        if self.phase != Some(progress.phase)
            || progress.completed_files.saturating_sub(self.last_files) >= 100
        {
            writeln!(
                self.stderr,
                "{}: {} files",
                progress.phase, progress.completed_files
            )
            .map_err(|error| Error::caused_by("write progress", error))?;
            self.phase = Some(progress.phase);
            self.last_files = progress.completed_files;
        }
        Ok(())
    }
}
