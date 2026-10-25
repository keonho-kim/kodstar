// Purpose: 공통 CLI 옵션·도움말과 숫자·크기·정의 위치 입력 파싱.
// Pattern: Simple Module.
// Usage: 명령 구성 모듈이 의미 있는 옵션만 부착.
// Related: crates/kodstar-cli/src/args.rs, crates/kodstar-cli/src/args/queries.rs.

use clap::{Arg, ArgAction, Command};
use kodstar_core::Settings;
use std::path::PathBuf;

pub(super) fn flag(name: &'static str, help: &'static str) -> Arg {
    Arg::new(name)
        .long(name)
        .action(ArgAction::SetTrue)
        .help(help)
}

pub(super) fn config(command: Command) -> Command {
    command.arg(
        Arg::new("config")
            .long("config")
            .value_parser(clap::value_parser!(PathBuf))
            .help("Explicit TOML configuration file")
            .long_help("Read this TOML file after ~/.kodstar/config.toml; command-line settings override both. Relative \
                paths are resolved from the current directory. This explicitly selected file must exist."),
    )
}

pub(super) fn project(command: Command, analysis: bool) -> Command {
    let command = config(command).arg(
        Arg::new("root")
            .long("root")
            .value_parser(clap::value_parser!(PathBuf))
            .help("Explicit project root")
            .long_help("Select an absolute project directory or a path relative to the current directory. Otherwise \
                project commands discover a registered ancestor root. File arguments remain relative to the \
                current directory, not this option."),
    );
    if !analysis {
        return command;
    }
    let defaults = Settings::default();
    command
        .arg(
            Arg::new("jobs")
                .long("jobs")
                .value_parser(positive)
                .help("Analysis worker count (1-64)")
                .long_help(format!("Maximum analysis worker count, from 1 to 64. Built-in default: {}; TOML configuration can \
                    override it. Memory settings must cover all worker inputs and queued work.", defaults.jobs)),
        )
        .arg(
            Arg::new("memory-budget")
                .long("memory-budget")
                .value_parser(size)
                .help("Scheduling memory budget, for example 256MiB")
                .long_help(format!("Scheduling budget in bytes or B, KB, KiB, MB, MiB, GB, or GiB. Built-in default: {} bytes; TOML \
                    configuration can override it. This must cover configured workers, buffers, and DB caches. AST \
                    and allocator overhead are additional; this is not an OS-enforced RSS limit.", defaults.memory_budget)),
        )
        .arg(
            Arg::new("max-file-bytes")
                .long("max-file-bytes")
                .value_parser(positive)
                .help("Maximum analysis input bytes per file")
                .long_help(format!("Positive analysis input limit in bytes per file. Built-in default: {}; TOML configuration can \
                    override it. Larger inputs are reported as skipped in analysis coverage; this does not discard \
                    preserved source history.", defaults.max_file_bytes)),
        )
}

pub(super) fn format(command: Command) -> Command {
    command.arg(
        Arg::new("format")
            .long("format")
            .value_parser(["text", "json", "jsonl", "tsv"])
            .default_value("text")
            .help("Result output format")
            .long_help("Choose text, JSON, newline-delimited JSON, or tab-separated values for results on stdout. \
                Progress and diagnostics go to stderr. The stdio session has its own protocol envelope."),
    )
}

pub(super) fn limits(command: Command) -> Command {
    command
        .arg(
            Arg::new("limit")
                .long("limit")
                .value_parser(positive)
                .conflicts_with("all")
                .help("Maximum output rows; conflicts with --all")
                .long_help(format!("Positive output row limit. Built-in default: {}; TOML configuration can override it. Reaching a \
                    result limit is reported as truncation with exit code 3. This is separate from traversal depth \
                    and --max-nodes.", Settings::default().limit)),
        )
        .arg(flag("all", "Remove the output row limit")
            .long_help("Remove only the output row limit. Traversal depth, visited-node, input, and memory limits still \
                apply. Conflicts with --limit."))
}

pub(super) fn refresh(command: Command) -> Command {
    command
        .arg(
            flag(
                "no-refresh",
                "Skip source checks, automatic checkpoints, and analysis refresh",
            )
            .long_help("Read saved results without checking source changes, creating automatic checkpoints, or updating \
                analysis. Freshness is reported as not_checked. Analysis queries need a compatible committed \
                cache. Conflicts with --verify.")
            .conflicts_with("verify"),
        )
        .arg(flag(
            "verify",
            "Verify source content hashes during synchronization",
        ).long_help("Check tracked file content hashes during current-state synchronization instead of relying only \
            on metadata. This can capture changes and update analysis before the result is read. Conflicts \
            with --no-refresh. Explicit checkpoint comparisons and --rev queries read stored sources instead \
            of checking working files."))
}

pub(super) fn location(command: Command) -> Command {
    command.arg(
        Arg::new("at")
            .long("at")
            .value_parser(at)
            .help("Select a definition at line:byte-column, both starting at 1")
            .long_help("Disambiguate a symbol by its definition start, for example 24:1. Lines and UTF-8 byte columns \
                start at 1; columns are not character counts or UTF-16 units. impact requires a symbol name when \
                --at is used."),
    )
}

pub(super) fn path(name: &'static str, required: bool) -> Arg {
    Arg::new(name)
        .required(required)
        .value_parser(clap::value_parser!(PathBuf))
        .help(if name == "file" {
            "Source file inside the selected project, relative to the current directory or absolute"
        } else {
            "Filesystem path, relative to the current directory or absolute"
        })
}

fn positive(value: &str) -> Result<usize, String> {
    let value: usize = value
        .parse()
        .map_err(|_| "expected a positive integer".to_owned())?;
    if value == 0 {
        return Err("expected a positive integer".into());
    }
    Ok(value)
}

fn at(value: &str) -> Result<(usize, usize), String> {
    let (line, column) = value.split_once(':').ok_or("expected line:column")?;
    Ok((positive(line)?, positive(column)?))
}

fn size(value: &str) -> Result<usize, String> {
    let value = value.trim();
    let split = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let (quantity, suffix) = value.split_at(split);
    let quantity = positive(quantity)?;
    let factor = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1000,
        "ki" | "kib" => 1024,
        "m" | "mb" => 1_000_000,
        "mi" | "mib" => 1024 * 1024,
        "g" | "gb" => 1_000_000_000,
        "gi" | "gib" => 1024 * 1024 * 1024,
        _ => return Err("expected B, KB, KiB, MB, MiB, GB, or GiB".into()),
    };
    quantity
        .checked_mul(factor)
        .ok_or_else(|| "byte size overflows this platform".into())
}
