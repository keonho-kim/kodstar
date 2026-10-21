// Purpose: 공통 CLI 옵션과 숫자·크기·정의 위치 입력 파싱.
// Pattern: Simple Module.
// Usage: 명령 구성 모듈이 의미 있는 옵션만 부착.
// Related: crates/kodstar-cli/src/args.rs, crates/kodstar-cli/src/args/queries.rs.

use clap::{Arg, ArgAction, Command};
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
            .help("Explicit TOML configuration"),
    )
}

pub(super) fn project(command: Command, analysis: bool) -> Command {
    let command = config(command).arg(
        Arg::new("root")
            .long("root")
            .value_parser(clap::value_parser!(PathBuf))
            .help("Explicit project root"),
    );
    if !analysis {
        return command;
    }
    command
        .arg(
            Arg::new("jobs")
                .long("jobs")
                .value_parser(positive)
                .help("Analysis worker count"),
        )
        .arg(
            Arg::new("memory-budget")
                .long("memory-budget")
                .value_parser(size)
                .help("Scheduling budget, for example 256MiB"),
        )
        .arg(
            Arg::new("max-file-bytes")
                .long("max-file-bytes")
                .value_parser(positive)
                .help("Maximum source input bytes per file"),
        )
}

pub(super) fn format(command: Command) -> Command {
    command.arg(
        Arg::new("format")
            .long("format")
            .value_parser(["text", "json", "jsonl", "tsv"])
            .default_value("text"),
    )
}

pub(super) fn limits(command: Command) -> Command {
    command
        .arg(
            Arg::new("limit")
                .long("limit")
                .value_parser(positive)
                .conflicts_with("all")
                .help("Maximum output rows"),
        )
        .arg(flag("all", "Remove the output row limit"))
}

pub(super) fn refresh(command: Command) -> Command {
    command
        .arg(
            flag(
                "no-refresh",
                "Read the last committed index without checking sources",
            )
            .conflicts_with("verify"),
        )
        .arg(flag(
            "verify",
            "Verify source content hashes before querying",
        ))
}

pub(super) fn location(command: Command) -> Command {
    command.arg(
        Arg::new("at")
            .long("at")
            .value_parser(at)
            .help("Definition start at line:byte-column, both starting at 1"),
    )
}

pub(super) fn path(name: &'static str, required: bool) -> Arg {
    Arg::new(name)
        .required(required)
        .value_parser(clap::value_parser!(PathBuf))
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
