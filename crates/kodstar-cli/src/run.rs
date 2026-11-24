// Purpose: CLI 요청에 맞는 엔진 호출과 구조화된 결과 전달.
// Pattern: Composition Root.
// Usage: main이 파싱된 Invocation을 실행.
// Related: crates/kodstar-cli/src/invocation.rs, crates/kodstar-cli/src/output.rs, crates/kodstar-core/src/application/engine.rs.

use crate::invocation::{Action, Invocation};
use crate::output::Renderer;
use kodstar_core::{Engine, Error, QuerySummary, Result};
use std::io;
use std::process::ExitCode;

pub(crate) fn execute(mut invocation: Invocation, verbose: bool) -> Result<ExitCode> {
    match invocation.action {
        Action::Install => return crate::installation::install(),
        Action::Uninstall => return crate::installation::uninstall(invocation.yes),
        _ => {}
    }
    if matches!(invocation.action, Action::Serve) {
        let _lease = Engine::from_environment()?.hold_local_data(&crate::signals::cancellation())?;
        let _signals = crate::signals::SignalGuard::install()?;
        crate::session::run(&crate::signals::cancellation())?;
        return Ok(ExitCode::SUCCESS);
    }
    let stdout = io::stdout();
    let writer = io::BufWriter::with_capacity(64 * 1024, stdout.lock());
    let mut sink = Renderer::new(writer, invocation.format, verbose);
    let mut project = invocation
        .project
        .ok_or_else(|| Error::argument("project options are required"))?;
    let engine = Engine::from_environment()?;
    if let Action::Init(options) = &mut invocation.action {
        let target = engine.initialization_target(&project, options)?;
        if !target.initialized {
            crate::confirmation::ask(
                &format!(
                    "Initialize KodStar in {}? Source changes will be stored in this project's .kodstar history.",
                    target.root.display()
                ),
                invocation.yes,
            )?;
        }
        project.root = Some(target.root.clone());
        options.path = Some(target.root);
    }
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
