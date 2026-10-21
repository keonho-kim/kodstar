// Purpose: 조회 명령별 위치 인자와 유효한 관계·탐색 옵션 구성.
// Pattern: Simple Module.
// Usage: 최상위 CLI 명령 구성이 조회 subcommand를 등록.
// Related: crates/kodstar-cli/src/args.rs, crates/kodstar-cli/src/args/options.rs.

use super::options::{self, flag};
use clap::{Arg, ArgAction, Command};
use kodstar_core::{Direction, RelationKind};

pub(super) fn commands() -> Vec<Command> {
    let symbols = base(
        Command::new("symbols")
            .about("List definitions in a file")
            .arg(options::path("file", true)),
    );
    let list = relations(
        base(
            Command::new("list")
                .about("List directly connected files")
                .arg(options::path("file", true)),
        ),
        "list",
    );
    let search = base(
        Command::new("search")
            .about("Find object definitions by name")
            .arg(Arg::new("name").required(true))
            .arg(flag("prefix", "Match a name prefix")),
    );
    let mut commands = vec![symbols, list, search];
    for (name, about) in [
        ("related", "Show incoming and outgoing relationships"),
        ("deps", "Show outgoing dependencies"),
        ("refs", "Show incoming references"),
        ("explain", "Explain definitions and relationship evidence"),
    ] {
        let command = options::location(base(
            Command::new(name)
                .about(about)
                .arg(options::path("file", true))
                .arg(Arg::new("name").required(true)),
        ));
        let command = command.arg(flag("members", "Include owned members as traversal seeds"));
        let command = if name == "explain" {
            command
        } else {
            relations(command, name)
        };
        let command = if name == "explain" {
            command
        } else {
            traversal(command)
        };
        commands.push(command.arg(flag("sites", "Show individual use locations")));
    }
    commands.push(
        traversal(relations(
            options::location(base(
                Command::new("impact")
                    .about("Find incoming static impact candidates")
                    .arg(options::path("file", true))
                    .arg(Arg::new("name")),
            )),
            "impact",
        ))
        .arg(flag("sites", "Show individual use locations")),
    );
    commands.push(
        traversal(relations(
            base(
                Command::new("path")
                    .about("Find a shortest directed relationship path")
                    .arg(Arg::new("from").required(true))
                    .arg(Arg::new("to").required(true)),
            ),
            "path",
        ))
        .arg(flag("sites", "Show individual use locations")),
    );
    commands
}

fn base(command: Command) -> Command {
    options::format(options::limits(options::refresh(options::project(
        command, true,
    ))))
}

fn relations(command: Command, name: &str) -> Command {
    let out_only = name == "deps";
    let in_only = matches!(name, "refs" | "impact");
    let direction = Arg::new("direction")
        .long("direction")
        .value_parser(move |value: &str| {
            let direction: Direction = value
                .parse()
                .map_err(|error: kodstar_core::Error| error.to_string())?;
            if out_only && direction != Direction::Out || in_only && direction != Direction::In {
                return Err("direction conflicts with this command".to_owned());
            }
            Ok(direction)
        });
    command
        .arg(direction)
        .arg(
            Arg::new("relation")
                .long("relation")
                .action(ArgAction::Append)
                .value_parser(clap::value_parser!(RelationKind)),
        )
        .arg(flag(
            "include-candidates",
            "Include ambiguous and candidate relationships",
        ))
        .arg(flag("include-external", "Include external targets"))
}

fn traversal(command: Command) -> Command {
    command
        .arg(
            Arg::new("depth")
                .long("depth")
                .value_parser(clap::value_parser!(std::num::NonZeroUsize))
                .conflicts_with("transitive"),
        )
        .arg(flag("transitive", "Traverse without a depth bound"))
        .arg(
            Arg::new("max-nodes")
                .long("max-nodes")
                .value_parser(clap::value_parser!(usize))
                .help("Maximum visited objects; 0 removes this bound"),
        )
}
