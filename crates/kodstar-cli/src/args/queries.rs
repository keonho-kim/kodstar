// Purpose: 조회 명령의 인자·탐색 옵션과 동작·예시 안내.
// Pattern: Simple Module.
// Usage: 최상위 CLI 명령 구성이 조회 subcommand를 등록.
// Related: crates/kodstar-cli/src/args.rs, crates/kodstar-cli/src/args/options.rs.

use super::options::{self, flag};
use clap::{Arg, ArgAction, Command};
use kodstar_core::{Direction, RelationKind, Settings};

pub(super) fn commands() -> Vec<Command> {
    let symbols = base(
        Command::new("symbols")
            .about("List definitions in a file")
            .long_about("List definitions with original names, kinds, scopes, and definition locations in an indexed source file.")
            .after_long_help("Examples:\n  kodstar symbols src/api.rs\n  kodstar symbols src/api.rs --rev baseline --format json")
            .arg(options::path("file", true)),
    );
    let list = relations(
        base(
            Command::new("list")
                .about("List directly connected files")
                .long_about("List other files connected by imports, includes, or object relationships, with direction, kinds, \
                    and counts. Both incoming and outgoing connections are included by default. Same-file \
                    relationships are excluded; this is a direct file query, not recursive traversal.")
                .after_long_help("Examples:\n  kodstar list src/api.rs\n  kodstar list src/api.rs --direction out --relation imports")
                .arg(options::path("file", true)),
        ),
        "list",
    );
    let search = base(
        Command::new("search")
            .about("Find object definitions by name")
            .long_about("Find definitions by exact name or prefix using language-specific name rules. Use --ranked to \
                match words in identifiers, qualified names and paths, then show direct relationships for up to \
                eight leading definitions. Definitions stay distinct by language, scope, and location.")
            .after_long_help("Examples:\n  kodstar search target\n  kodstar search Service --prefix --format json\n  kodstar search StoreOrder --ranked")
            .arg(Arg::new("name").required(true)
                .help("Definition name, prefix or words"))
            .arg(flag("prefix", "Match a name prefix"))
            .arg(flag("ranked", "Rank definitions and show direct relationships").conflicts_with("prefix"))
            .arg(visited_limit().requires("ranked")
                .help("Maximum matching definitions scored (0 for unlimited)")
                .long_help("Limit how many definitions with at least one matching word or word prefix are scored by ranked search. A value of 0 removes this bound.")),
    );
    let overview = base(
        Command::new("overview")
            .about("Summarize project structure and important dependencies")
            .long_about("List source directories, connections across directories and package boundaries, objects with many \
                distinct users, and groups of files that participate in import cycles. Use a single index generation and resolved \
                internal relationships. File, relationship and output bounds can produce a partial report.")
            .after_long_help("Examples:\n  kodstar overview\n  kodstar overview --rev baseline --format jsonl")
            .arg(visited_limit().help("Maximum indexed files examined; 0 removes this bound")
                .long_help("Limit the indexed files included in the report. The memory budget can further limit files and relationships.")),
    );
    let mut commands = vec![symbols, list, search, overview];
    for (name, about, details, examples) in [
        (
            "related",
            "Show incoming and outgoing relationships",
            "Show relationships in both directions by default, preserving incoming versus outgoing labels. \
                Default depth is 1. Transitive both-direction queries combine separate incoming and outgoing \
                reachability rather than changing direction at each step.",
            "Examples:\n  kodstar related src/api.rs caller\n  kodstar related src/api.rs Service --members --depth 3",
        ),
        (
            "deps",
            "Show outgoing dependencies",
            "Show dependencies leaving the selected symbol. Only the out direction is accepted; default depth \
                is 1. Use --relation to filter kinds or --transitive to follow further dependencies within the \
                configured limits.",
            "Examples:\n  kodstar deps src/api.rs caller\n  kodstar deps src/api.rs caller --relation calls --sites",
        ),
        (
            "refs",
            "Show incoming references",
            "Show references entering the selected symbol. Only the in direction is accepted; default depth \
                is 1. Use --sites for individual use locations or --transitive for callers further upstream.",
            "Examples:\n  kodstar refs src/api.rs target --sites\n  kodstar refs src/api.rs target --transitive --format json",
        ),
        (
            "explain",
            "Explain definitions and relationship evidence",
            "Show definition information and direct relationship evidence, including use locations, \
                resolution rules, candidates, and unresolved reasons. Ambiguous symbol selections require a \
                qualified name or --at; the first match is not silently chosen.",
            "Examples:\n  kodstar explain src/api.rs caller\n  kodstar explain src/api.rs Service::run --at 24:1",
        ),
    ] {
        let command = options::location(base(
            Command::new(name)
                .about(about)
                .long_about(details)
                .after_long_help(examples)
                .arg(options::path("file", true))
                .arg(symbol(true)),
        ));
        let command = command.arg(flag("members", "Include owned members as traversal seeds")
            .long_help("Include members owned by the selected type or class as additional relationship seeds. Without \
                this flag, start from the selected symbol alone."));
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
                    .long_about("Find upstream static impact candidates from a file or selected symbol, or from changes since \
                        --since. Direction is in and traversal has no default depth bound, while output and visited-node \
                        limits still apply.\n\n--since compares saved source states and uses changed definitions and \
                        relationships from both sides, including deleted definitions. It conflicts with file/name seeds, \
                        --at, and --rev. Results describe static candidates rather than guaranteed runtime effects.")
                    .after_long_help("Examples:\n  kodstar impact src/api.rs target --depth 3\n  kodstar impact --since baseline --format json")
                    .arg(options::path("file", false).required_unless_present("since"))
                    .arg(symbol(false))
                    .arg(
                        Arg::new("since")
                            .long("since")
                            .help("Use changes since this checkpoint ID or saved name as impact seeds")
                            .conflicts_with_all(["file", "name", "at", "rev"]),
                    ),
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
                    .long_about("Find one shortest directed path between two file::qualified-name selectors using the selected \
                        directions and relationship filters. Both directions are allowed by default, with no default \
                        depth bound; result and visited-node limits still apply. --undirected allows a direction change \
                        between hops while showing each edge's actual direction. Preserve :: inside a qualified symbol name.")
                    .after_long_help("Examples:\n  kodstar path src/api.rs::caller src/db.rs::save\n  kodstar path src/api.rs::caller src/db.rs::other --undirected --relation calls")
                    .arg(flag("undirected", "Allow direction changes between hops").conflicts_with("direction"))
                    .arg(Arg::new("from").required(true)
                        .help("Start selector: file::qualified-name; file is relative to the current directory or absolute"))
                    .arg(Arg::new("to").required(true)
                        .help("Destination selector: file::qualified-name; file is relative to the current directory or absolute")),
            ),
            "path",
        ))
        .arg(flag("sites", "Show individual use locations")),
    );
    commands
}

fn base(command: Command) -> Command {
    let details = command
        .get_long_about()
        .map(ToString::to_string)
        .unwrap_or_default();
    let command = command.long_about(format!("{details}\n\nCurrent-state queries preserve observed source changes as checkpoints and \
        synchronize analysis before reading. --no-refresh skips those checks. --rev reads preserved \
        source at a checkpoint without switching files or capturing current edits; analysis for that \
        checkpoint may be built unless --no-refresh is used."));
    options::format(options::limits(options::refresh(options::project(
        command.arg(
            Arg::new("rev")
                .long("rev")
                .help("Read analysis at a checkpoint ID or saved name")
                .long_help("Analyze preserved sources at this checkpoint without changing working files or the current \
                    branch. Uses current engine settings, not a historical toolchain. With --no-refresh, compatible \
                    cached analysis for this checkpoint must already exist. Conflicts with impact --since."),
        ),
        true,
    ))))
}

fn relations(command: Command, name: &str) -> Command {
    let out_only = name == "deps";
    let in_only = matches!(name, "refs" | "impact");
    let direction = Arg::new("direction")
        .long("direction")
        .help(if out_only {
            "Relationship direction: out (default and only choice)"
        } else if in_only {
            "Relationship direction: in (default and only choice)"
        } else {
            "Relationship direction: in, out, or both (default)"
        })
        .value_parser(move |value: &str| {
            let direction: Direction = value
                .parse()
                .map_err(|error: kodstar_core::Error| error.to_string())?;
            if out_only && direction != Direction::Out
                || in_only && direction != Direction::In
                || direction == Direction::Undirected
            {
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
                .help("Filter by relationship kind; repeat to include multiple kinds")
                .long_help(format!("Include the union of the requested relationship kinds. Available kinds: {}. Omit this option to \
                    use the command's default relationship selection.", RelationKind::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>().join(", ")))
                .value_parser(clap::value_parser!(RelationKind)),
        )
        .arg(flag(
            "include-candidates",
            "Include ambiguous and candidate relationships",
        ).long_help("Include candidate and ambiguous relationships alongside resolved ones. \
            Default relationship queries select resolved results. This does not turn candidates \
            into confirmed targets; use explain to inspect unresolved evidence."))
        .arg(flag("include-external", "Include external targets")
            .long_help("Include external package, module, or symbol targets where the query supports them. \
                Default relationship queries select internal targets. Resolution filters still apply."))
}

fn traversal(command: Command) -> Command {
    let depth = if matches!(command.get_name(), "impact" | "path") {
        "unbounded"
    } else {
        "1"
    };
    command
        .arg(
            Arg::new("depth")
                .long("depth")
                .help("Maximum traversal depth; positive integer, conflicts with --transitive")
                .long_help(format!("Maximum relationship depth from the selected seeds. Default for this command: {depth}. This does \
                    not replace output-row or visited-node limits. Conflicts with --transitive."))
                .value_parser(clap::value_parser!(std::num::NonZeroUsize))
                .conflicts_with("transitive"),
        )
        .arg(flag("transitive", "Traverse without a depth bound")
            .long_help("Remove the depth bound while retaining the output-row and visited-node limits. Conflicts with --depth."))
        .arg(visited_limit())
}

fn visited_limit() -> Arg {
    Arg::new("max-nodes")
        .long("max-nodes")
        .value_parser(clap::value_parser!(usize))
        .help("Maximum visited objects; 0 removes this bound")
        .long_help(format!("Limit visited objects separately from output rows and depth. Built-in default: {}; TOML \
            configuration can override it. A value of 0 removes this bound; traversal still uses its \
            remaining limits.", Settings::default().max_nodes))
}

fn symbol(required: bool) -> Arg {
    let optional = if required {
        ""
    } else {
        " Omitting the name selects the file's definitions."
    };
    Arg::new("name").required(required)
        .help("Symbol name; use a qualified name or --at to disambiguate")
        .long_help(format!("Original symbol name or owner-qualified name in the selected file, for example Service::run. If \
            several definitions match, select a qualified name or its definition position with --at.{optional}"))
}
