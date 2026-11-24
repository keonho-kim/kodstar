// Purpose: Identify recoverable source failures without conflating execution failures.
// Pattern: Domain Value.
// Usage: Parser adapters attach an issue; analysis workers isolate its file.
// Related: crates/kodstar-core/src/domain/diagnostics.rs, crates/kodstar-core/src/application/analysis/workers.rs.

use super::Error;

#[derive(Clone, Debug)]
pub(crate) struct InputIssue {
    pub code: &'static str,
    pub line: Option<usize>,
}

impl Error {
    pub(crate) fn input(
        code: &'static str,
        message: impl Into<String>,
        line: Option<usize>,
    ) -> Self {
        let mut error = Self::operation(message);
        error.input = Some(InputIssue { code, line });
        error
    }
}
