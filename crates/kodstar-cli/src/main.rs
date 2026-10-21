// Purpose: 인자 파싱·실행·프로세스 종료 코드 연결.
// Pattern: Composition Root.
// Usage: kodstar 실행 파일.
// Related: crates/kodstar-cli/src/args.rs, crates/kodstar-cli/src/invocation.rs, crates/kodstar-cli/src/run.rs, crates/kodstar-cli/src/output.rs.

mod args;
mod invocation;
mod output;
mod run;
mod session;
mod signals;
use std::process::ExitCode;

fn main() -> ExitCode {
    let matches = match args::command().try_get_matches() {
        Ok(matches) => matches,
        Err(error) => return output::render_argument_result(error),
    };
    match invocation::Invocation::parse(&matches).and_then(run::execute) {
        Ok(exit) => exit,
        Err(error) => output::render_error(&error),
    }
}
