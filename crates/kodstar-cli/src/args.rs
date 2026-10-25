// Purpose: CLI 명령 트리와 짧은·상세 도움말·버전 구성.
// Pattern: Simple Module.
// Usage: main이 clap 명령을 생성.
// Related: crates/kodstar-cli/src/main.rs, crates/kodstar-cli/src/args/options.rs, crates/kodstar-cli/src/args/queries.rs.

mod history;
mod options;
mod queries;
#[cfg(test)]
mod tests;
use clap::Command;

pub(crate) fn command() -> Command {
    let initialize = options::format(options::project(
        Command::new("init")
            .about("Initialize local history and analyze the captured source state")
            .long_about("Create local source history and an initial analysis index. Later runs preserve observed changes \
                and update the analysis incrementally.\n\nFirst initialization asks you to confirm the target \
                directory; non-interactive use requires --yes. The filesystem root is refused. Existing \
                checkpoint history is preserved when rebuilding analysis.")
            .after_long_help("Examples:\n  kodstar init .\n  kodstar init /work/project --yes --jobs 2 --memory-budget 256MiB\n  kodstar init . --full")
            .arg(options::flag(
                "yes",
                "Confirm the displayed initialization target without prompting",
            ))
            .arg(options::path("path", false)
                .help("Project directory; defaults to the current directory")
                .long_help("Directory to initialize or update, relative to the current directory or absolute. If --root is \
                    also provided, both must resolve to the same directory."))
            .arg(options::flag(
                "full",
                "Re-extract every source into a new committed generation",
            ))
            .arg(options::flag("verify", "Verify source content hashes")
                .long_help("Verify tracked file content hashes instead of relying only on metadata to detect changes. This \
                    does not force re-extraction of unchanged files; use --full for that.")),
        true,
    ));
    let mut command = Command::new("kodstar")
        .about("Local source-code relationship analysis")
        .long_about("Explore code relationships and preserve observed source changes as local checkpoints.\n\nRun \
            init in a project first. Regular project queries synchronize changes before reading the index; \
            --no-refresh skips that synchronization. Help and version output do not initialize a project or \
            create application data.\n\nUse -h for a summary and --help for detailed behavior, options, and \
            examples.")
        .after_long_help("Examples:\n  kodstar init .\n  kodstar deps src/api.rs caller --sites\n  kodstar history --help")
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
                .long_about("After confirmation, remove the installed executable, managed shell PATH entries, default global \
                    settings, caches, and checkpoint history for ALL registered projects. Original source files, \
                    unrelated shell settings, development checkouts, build outputs, and Rust toolchains are \
                    preserved.\n\nActive sessions, unknown data, or modified installation records stop removal with \
                    a diagnostic. If a project moved, use KodStar at its new location before uninstalling.")
                .after_long_help("Examples:\n  kodstar uninstall\n  kodstar uninstall --yes")
                .arg(options::flag(
                    "yes",
                    "Confirm complete removal without prompting",
                )),
        )
        .subcommand(
            Command::new("serve")
                .about("Serve versioned stdio requests for external tools")
                .long_about("Run a protocol 2 session for external tools using newline-delimited JSON on stdin/stdout. \
                    Results, progress, and completion events go to stdout; process diagnostics go to stderr.\n\nThe \
                    session owns its worker and cancellation lifecycle. Close stdin or send a shutdown request to \
                    finish. Merely starting the session does not initialize a source project.")
                .after_long_help("Examples:\n  kodstar serve --stdio\n  kodstar serve --stdio < requests.jsonl")
                .arg(
                    options::flag("stdio", "Use line-delimited JSON requests on stdin/stdout")
                        .required(true),
                ),
        )
        .subcommand(options::format(options::project(
            Command::new("watch")
                .about("Watch saved files and publish successful updates until interrupted")
                .long_about("Watch saved files in an initialized project. Each synchronization preserves observed source \
                    changes as a checkpoint and updates analysis; duplicate events without changes add no \
                    checkpoint.\n\nPress Ctrl-C to stop. Events are checked against filesystem state, and missed \
                    events trigger a rescan. This runs only while the command is active; no service is installed.")
                .after_long_help("Examples:\n  kodstar watch\n  kodstar watch --root /work/project --format jsonl"),
            true,
        )))
        .subcommand(
            Command::new("cache")
                .about("Inspect and manage project caches")
                .long_about("Inspect or remove rebuildable analysis caches under ~/.kodstar. Cache cleanup preserves project \
                    source files and checkpoint history.\n\nPut options after the subcommand. Active cache leases \
                    protect caches used by running operations.")
                .after_long_help("Examples:\n  kodstar cache list --format json\n  kodstar cache gc --dry-run")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(options::format(options::refresh(options::project(
                    Command::new("list")
                        .about("List analysis caches, sizes, and usage state")
                        .long_about("List registered analysis cache locations, sizes, and usage state. When an initialized project is \
                            selected, synchronize it first unless --no-refresh is set. Without a selected project, report \
                            caches without starting analysis.")
                        .after_long_help("Examples:\n  kodstar cache list\n  kodstar cache list --no-refresh --format json"),
                    true,
                ))))
                .subcommand(options::format(
                    options::project(Command::new("clean"), false)
                        .about("Remove inactive analysis caches while preserving source history")
                        .long_about("Remove the selected project's inactive analysis cache, or every inactive managed cache with \
                            --all. Source files and project checkpoint histories are preserved.\n\nCleanup does not \
                            synchronize source changes. --all conflicts with --root. The next regular project query can \
                            regenerate a removed compatible cache from preserved history and current files.")
                        .after_long_help("Examples:\n  kodstar cache clean --root /work/project\n  kodstar cache clean --all")
                        .arg(options::flag("all", "Clean every inactive managed cache"))
                        .mut_arg("root", |arg| arg.conflicts_with("all")),
                ))
                .subcommand(options::format(options::config(Command::new("gc")
                    .about("Remove eligible inactive caches by retention and size policy")
                    .long_about("Apply the configured retention and size policies to inactive analysis caches. Caches used by \
                        running operations are protected. Project sources and checkpoint history are preserved; no \
                        source synchronization is performed.\n\nUse --dry-run to list candidates and reasons without \
                        removing caches. Retention and size defaults can be overridden in the global or explicit TOML \
                        configuration.")
                    .after_long_help("Examples:\n  kodstar cache gc --dry-run\n  kodstar cache gc --format json")
                    .arg(
                    options::flag("dry-run", "Show deletion candidates without removing them"),
                )))),
        )
        .subcommands(queries::commands())
        .subcommands(history::commands())
        .subcommand(options::format(options::refresh(options::project(
            Command::new("languages")
                .about("Show actual language capabilities and limitations")
                .long_about("Report supported inputs, implemented static rules, and known limitations for each language and \
                    format. Grammar registration alone does not imply complete language support.\n\nSynchronize a \
                    selected initialized project first unless --no-refresh is set. Without a selected project, \
                    report global capabilities without creating a project.")
                .after_long_help("Examples:\n  kodstar languages\n  kodstar languages --format json"),
            true,
        ))));
    for (name, about, details, examples) in [
        (
            "status",
            "Show project history, analysis state, and freshness",
            "Show the selected root, storage locations, branch, checkpoint, analysis generation, freshness, \
                and coverage. Synchronize observed changes first unless --no-refresh is set.\n\nIf \
                synchronization fails, stored history and the last valid analysis can be reported with their \
                mismatch and failure diagnostics.",
            "Examples:\n  kodstar status\n  kodstar status --no-refresh --format json",
        ),
        (
            "doctor",
            "Check history and analysis integrity and report diagnostics",
            "Diagnose source history, analysis index integrity, format compatibility, project paths, and \
                analysis failures. Synchronize observed changes first unless --no-refresh is set.\n\nDiagnostics \
                distinguish stored history from the last valid analysis. This command does not silently upgrade \
                incompatible history or discard unknown data.",
            "Examples:\n  kodstar doctor\n  kodstar doctor --no-refresh --format json",
        ),
    ] {
        command = command.subcommand(options::format(options::refresh(options::project(
            Command::new(name)
                .about(about)
                .long_about(details)
                .after_long_help(examples),
            true,
        ))));
    }
    command
}
