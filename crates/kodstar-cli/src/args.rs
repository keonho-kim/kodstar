// Purpose: CLI 명령과 도움말·버전의 구성.
// Pattern: Simple Module.
// Usage: main이 clap 명령을 생성.
// Related: crates/kodstar-cli/src/main.rs, crates/kodstar-cli/src/args/options.rs, crates/kodstar-cli/src/args/queries.rs.

mod history;
mod options;
mod queries;
use clap::Command;

pub(crate) fn command() -> Command {
    let initialize = options::format(options::project(
        Command::new("init")
            .about("Initialize local history and analyze the captured source state")
            .arg(options::flag(
                "yes",
                "Confirm the displayed initialization target without prompting",
            ))
            .arg(options::path("path", false))
            .arg(options::flag(
                "full",
                "Re-extract every source into a new committed generation",
            ))
            .arg(options::flag("verify", "Verify source content hashes")),
        true,
    ));
    let mut command = Command::new("kodstar")
        .about("Local source-code relationship analysis")
        .version(format!(
            "{} (engine {})",
            env!("CARGO_PKG_VERSION"),
            kodstar_core::VERSION
        ))
        .arg_required_else_help(true)
        .subcommand_required(true)
        .subcommand(initialize)
        .subcommand(Command::new("__install").hide(true))
        .subcommand(
            Command::new("uninstall")
                .about("Remove installation, settings, caches and registered project histories")
                .arg(options::flag(
                    "yes",
                    "Confirm complete removal without prompting",
                )),
        )
        .subcommand(
            Command::new("serve")
                .about("Run a versioned editor session")
                .arg(
                    options::flag("stdio", "Use line-delimited JSON requests on stdin/stdout")
                        .required(true),
                ),
        )
        .subcommand(options::format(options::project(
            Command::new("watch")
                .about("Watch saved files and publish successful updates until interrupted"),
            true,
        )))
        .subcommand(
            Command::new("cache")
                .about("Inspect and manage project caches")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(options::format(options::refresh(options::project(
                    Command::new("list"),
                    true,
                ))))
                .subcommand(options::format(
                    options::project(Command::new("clean"), false)
                        .arg(options::flag("all", "Clean every inactive managed cache"))
                        .mut_arg("root", |arg| arg.conflicts_with("all")),
                ))
                .subcommand(options::format(options::config(Command::new("gc").arg(
                    options::flag("dry-run", "Show deletion candidates without removing them"),
                )))),
        )
        .subcommands(queries::commands())
        .subcommands(history::commands())
        .subcommand(options::format(options::refresh(options::project(
            Command::new("languages").about("Show actual language capabilities and limitations"),
            true,
        ))));
    for name in ["status", "doctor"] {
        command = command.subcommand(options::format(options::refresh(options::project(
            Command::new(name),
            true,
        ))));
    }
    command
}
