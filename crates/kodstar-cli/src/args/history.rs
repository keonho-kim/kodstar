// Purpose: 이력·브랜치 명령의 인자·도움말과 보존 동작 안내.
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
            .long_about("List checkpoints reachable from the current branch, newest first. By default, capture observed \
                source changes before listing; --no-refresh reads stored history only.\n\nHistory commands do \
                not require a successful analysis index. Put project and output options after the selected \
                subcommand.")
            .after_long_help("Examples:\n  kodstar history\n  kodstar history --no-refresh --format json")
            .args_conflicts_with_subcommands(true),
    ))
    .subcommand(base(
        Command::new("upgrade")
            .about("Verify, upgrade and repack preserved history storage")
            .long_about("Explicitly convert supported older history formats to the current format, or repack an existing \
                current-format repository. Verify preserved bytes and retain checkpoint IDs, names, and \
                branches.\n\nThis rewrites stored content under the repository lock and reclaims free pages. It \
                does not restore working files or capture new source changes. Incompatible unknown schemas are \
                rejected; failed conversion or repacking rolls back. Page reclamation is a separate step after \
                commit.")
            .after_long_help("Examples:\n  kodstar history upgrade\n  kodstar history upgrade --root /work/project --format json"),
    ))
    .subcommand(base(
        Command::new("save")
            .about("Capture current files and name their checkpoint")
            .long_about("Verify and preserve the current source state, then attach a name to its checkpoint. Unchanged \
                content reuses the current checkpoint.\n\nEach checkpoint has at most one name. An existing name \
                cannot be moved or overwritten; saving the same name at the same checkpoint is allowed. Analysis \
                is not required.")
            .after_long_help("Examples:\n  kodstar history save baseline\n  kodstar history save before-refactor --root /work/project")
            .arg(name("Name to attach to the captured checkpoint")),
    ))
    .subcommand(options::refresh(base(
        Command::new("diff")
            .about("Compare checkpoint source files")
            .long_about("By default, capture observed source changes and compare the resulting checkpoint with its \
                parent; the first checkpoint is compared with an empty tracked state. --no-refresh skips \
                capture.\n\nProvide --from and --to together to compare two stored checkpoint IDs or names \
                without capturing current changes. Report added, modified, or deleted files and bounded text \
                differences; binary or oversized text has an omission reason. Working files are not changed.")
            .after_long_help("Examples:\n  kodstar history diff\n  kodstar history diff --from baseline --to after-edit")
            .arg(Arg::new("from").long("from").requires("to")
                .help("Before checkpoint ID or saved name; requires --to"))
            .arg(Arg::new("to").long("to").requires("from")
                .help("After checkpoint ID or saved name; requires --from")),
    )))
    .subcommand(base(
        Command::new("restore")
            .about("Restore tracked files while preserving current changes")
            .long_about("Preserve current source changes first, then restore all tracked files or the selected path from \
                a checkpoint. Files absent from the target are removed within the selected tracked scope. \
                Untracked conflicts stop the operation.\n\nRestored content becomes a new checkpoint on the \
                current branch; earlier history is retained. Interrupted restores resume through a journal and \
                reject conflicting external edits.")
            .after_long_help("Examples:\n  kodstar history restore baseline src/api.rs\n  kodstar history restore baseline")
            .arg(Arg::new("revision").required(true)
                .help("Checkpoint ID (for example c12) or saved name to restore"))
            .arg(options::path("path", false)
                .help("Tracked path to restore; omitted restores all tracked files")),
    ));
    let branch = base(
        Command::new("branch")
            .about("List local branches")
            .long_about("List local branches, their checkpoints, and the current branch. Listing creates no checkpoint \
                and does not switch source files.\n\nBranches point to checkpoints rather than copying the \
                source tree or analysis DB. Creating and switching branches preserve current changes first. \
                Branch merging is not provided.")
            .after_long_help("Examples:\n  kodstar branch\n  kodstar branch --format json")
            .args_conflicts_with_subcommands(true),
    )
    .subcommands(
        [
            ("create", "Create a branch at the captured current state",
             "Verify and preserve current source changes, then create a branch at that checkpoint. The current \
                 branch and working files are not switched. The new branch name must not already exist.",
             "Examples:\n  kodstar branch create experiment\n  kodstar branch create refactor --root /work/project"),
            ("switch", "Preserve current changes and switch branch",
             "Preserve current source changes before switching to an existing branch. Restore its tracked \
                 files through the checkout journal and stop on untracked-file or external-edit conflicts. Failed \
                 preservation leaves the working files unswitched.",
             "Examples:\n  kodstar branch switch experiment\n  kodstar branch switch main --root /work/project"),
            ("delete", "Delete a non-current branch reference",
             "Delete an existing branch reference other than the current branch. This does not change working \
                 files, capture new source changes, or automatically delete its preserved checkpoints and names.",
             "Examples:\n  kodstar branch delete experiment\n  kodstar branch delete refactor --root /work/project"),
        ]
        .into_iter()
        .map(|(command, about, details, examples)| {
            base(
                Command::new(command)
                    .about(about)
                    .long_about(details)
                    .after_long_help(examples)
                    .arg(name("Branch name")),
            )
        }),
    );
    [history, branch]
}

fn name(help: &'static str) -> Arg {
    Arg::new("name").required(true).help(help)
        .long_help(format!("{help}. Use 1-128 ASCII letters, digits, dots, underscores, or hyphens, starting with a letter \
            or digit. Names matching c followed by digits are reserved for checkpoint IDs."))
}
