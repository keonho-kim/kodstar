// Purpose: CLI 요청에 맞는 엔진 호출과 구조화된 결과 전달.
// Pattern: Composition Root.
// Usage: main이 파싱된 Invocation을 실행.
// Related: crates/kodstar-cli/src/invocation.rs, crates/kodstar-cli/src/output.rs, crates/kodstar-core/src/application/engine.rs.

use crate::invocation::{Action, Invocation};
use crate::output::Renderer;
use kodstar_core::{Engine, Error, QuerySummary, Result};
use std::io;
use std::process::ExitCode;

pub(crate) fn execute(invocation: Invocation) -> Result<ExitCode> {
    if matches!(invocation.action, Action::Serve) {
        let _signals = crate::signals::SignalGuard::install()?;
        crate::session::run(&crate::signals::cancellation())?;
        return Ok(ExitCode::SUCCESS);
    }
    let stdout = io::stdout();
    let mut sink = Renderer::new(stdout.lock(), invocation.format);
    let project = invocation
        .project
        .ok_or_else(|| Error::argument("project options are required"))?;
    let engine = Engine::from_environment()?;
    let _signals = crate::signals::SignalGuard::install()?;
    let cancel = crate::signals::cancellation();
    let summary = engine.execute(&project, invocation.action.command()?, &cancel, &mut sink)?;
    Ok(exit(&summary))
}

fn exit(summary: &QuerySummary) -> ExitCode {
    if summary.complete && !summary.truncated {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}
