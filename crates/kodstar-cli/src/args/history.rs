// Purpose: history·branch의 최소 명령 트리와 옵션 구성.
// Pattern: Boundary Parser.
// Usage: 최상위 명령 구성이 로컬 이력 기능을 등록.
// Related: crates/kodstar-cli/src/invocation/history.rs.

use super::options;
use clap::{Arg, Command};

fn base(command: Command) -> Command {
    options::format(options::project(command, false))
}

pub(super) fn commands() -> [Command; 2] {
    let history = options::refresh(base(
        Command::new("history")
            .about("List local source checkpoints")
            .args_conflicts_with_subcommands(true),
    ))
    .subcommand(base(
        Command::new("upgrade").about("Verify, upgrade and repack preserved history storage"),
    ))
    .subcommand(base(
        Command::new("save")
            .about("Capture current files and name their checkpoint")
            .arg(Arg::new("name").required(true)),
    ))
    .subcommand(options::refresh(base(
        Command::new("diff")
            .about("Compare checkpoint source files")
            .arg(Arg::new("from").long("from").requires("to"))
            .arg(Arg::new("to").long("to").requires("from")),
    )))
    .subcommand(base(
        Command::new("restore")
            .about("Restore tracked files while preserving current changes")
            .arg(Arg::new("revision").required(true))
            .arg(options::path("path", false)),
    ));
    let branch = base(
        Command::new("branch")
            .about("List local branches")
            .args_conflicts_with_subcommands(true),
    )
    .subcommands(
        [
            ("create", "Create a branch at the captured current state"),
            ("switch", "Preserve current changes and switch branch"),
            ("delete", "Delete a non-current branch reference"),
        ]
        .into_iter()
        .map(|(name, about)| {
            base(
                Command::new(name)
                    .about(about)
                    .arg(Arg::new("name").required(true)),
            )
        }),
    );
    [history, branch]
}
