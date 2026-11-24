// Purpose: 인자 파싱·실행·프로세스 종료 코드 연결.
// Pattern: Composition Root.
// Usage: kodstar 실행 파일.
// Related: crates/kodstar-cli/src/args.rs, crates/kodstar-cli/src/invocation.rs, crates/kodstar-cli/src/run.rs, crates/kodstar-cli/src/output.rs, crates/kodstar-cli/src/telemetry.rs.

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("KodStar supports macOS and Linux only");

mod args;
mod confirmation;
mod installation;
mod invocation;
mod output;
mod run;
mod session;
mod signals;
mod telemetry;
use std::process::ExitCode;

fn main() -> ExitCode {
    let matches = match args::command().try_get_matches() {
        Ok(matches) => matches,
        Err(error) => return output::render_argument_result(error),
    };
    if matches.get_flag("licenses") {
        if matches.subcommand_name().is_some() {
            return output::render_argument_result(args::command().error(
                clap::error::ErrorKind::ArgumentConflict,
                "--licenses cannot be combined with a command",
            ));
        }
        return output::render_licenses();
    }
    let verbose = *matches
        .get_one::<bool>("verbose")
        .expect("global verbosity has a default");
    let _telemetry = telemetry::start(verbose);
    match invocation::Invocation::parse(&matches)
        .and_then(|invocation| run::execute(invocation, verbose))
    {
        Ok(exit) => exit,
        Err(error) => output::render_error(&error),
    }
}
