// Purpose: compile_commands의 분석용 arguments를 메모리 VFS 경로와 허용 flag로 파싱.
// Pattern: Boundary Parser.
// Usage: native session이 고정 체크포인트의 설정을 전달.
// Related: crates/kodstar-core/src/infrastructure/clang/inputs.rs, crates/kodstar-core/src/infrastructure/clang/mod.rs.

use super::inputs::ProjectInputs;
use crate::domain::diagnostics::Result;
use std::path::{Component, Path, PathBuf};
mod context;
mod database;
pub(super) use context::prepare;
pub(super) fn unavailable() -> context::Context {
    context::Context {
        arguments: None,
        owner: None,
    }
}
pub(super) use database::Database;

#[derive(serde::Deserialize)]
struct Command {
    file: String,
    #[serde(default)]
    directory: String,
    arguments: Option<Vec<String>>,
    command: Option<String>,
}

fn relative(root: &Path, directory: &str, value: &str) -> Option<PathBuf> {
    let base = if Path::new(directory).is_absolute() {
        Path::new(directory).strip_prefix(root).ok()?.to_path_buf()
    } else {
        PathBuf::from(directory)
    };
    let raw = if Path::new(value).is_absolute() {
        Path::new(value).strip_prefix(root).ok()?.to_path_buf()
    } else {
        base.join(value)
    };
    let mut path = PathBuf::new();
    for part in raw.components() {
        match part {
            Component::Normal(p) => path.push(p),
            Component::CurDir => {}
            Component::ParentDir if path.pop() => {}
            _ => return None,
        }
    }
    Some(path)
}

pub(super) fn arguments(
    input: &mut ProjectInputs,
    root: &Path,
    file: &Path,
    database: &mut Option<Database>,
) -> Result<Option<Vec<String>>> {
    let Some(_) = input.entry("compile_commands.json", false)? else {
        return Ok(Some(Vec::new()));
    };
    if database.is_none() {
        *database = Some(Database::load(input, root)?)
    }
    let Some(command) = database.as_ref().and_then(|d| d.select(file)) else {
        return Ok(Some(Vec::new()));
    };
    let Some(command) = command else {
        return Ok(None);
    };
    let args = command
        .arguments
        .clone()
        .or_else(|| command.command.as_deref().and_then(shlex::split));
    let Some(args) = args else {
        return Ok(None);
    };
    if args.iter().any(|a| a.starts_with('@') || a.contains('\0')) {
        return Ok(None);
    }
    if args.is_empty() || args.len() > 256 || args.iter().map(String::len).sum::<usize>() > 65536 {
        return Ok(None);
    }
    let mut output = Vec::new();
    let mut values = args.iter().skip(1);
    while let Some(arg) = values.next() {
        if arg == "-c" || arg.starts_with("-W") || matches!(arg.as_str(), "-g" | "-pipe") {
            continue;
        }
        if matches!(arg.as_str(), "-o" | "-MF" | "-MT" | "-MQ") {
            if values.next().is_none() {
                return Ok(None);
            }
            continue;
        }
        if arg.starts_with("-M") {
            continue;
        }
        if matches!(arg.as_str(), "-I" | "-isystem" | "-iquote" | "-include") {
            let Some(value) = values.next() else {
                return Ok(None);
            };
            let Some(path) = relative(root, &command.directory, value)
                .and_then(|p| p.to_str().map(str::to_owned))
            else {
                return Ok(None);
            };
            output.extend([arg.clone(), format!("/__kodstar_project/{path}")]);
            continue;
        }
        if let Some(value) = arg.strip_prefix("-I").filter(|s| !s.is_empty()) {
            let Some(path) = relative(root, &command.directory, value)
                .and_then(|p| p.to_str().map(str::to_owned))
            else {
                return Ok(None);
            };
            output.push(format!("-I/__kodstar_project/{path}"));
            continue;
        }
        if matches!(arg.as_str(), "-D" | "-U") {
            let Some(value) = values.next() else {
                return Ok(None);
            };
            output.extend([arg.clone(), value.clone()]);
            continue;
        }
        if arg.starts_with("-D")
            || arg.starts_with("-U")
            || arg.starts_with("-std=")
            || matches!(
                arg.as_str(),
                "-fno-exceptions"
                    | "-fno-rtti"
                    | "-fexceptions"
                    | "-frtti"
                    | "-pthread"
                    | "-O"
                    | "-O0"
                    | "-O1"
                    | "-O2"
                    | "-O3"
                    | "-Os"
                    | "-Oz"
                    | "-Og"
                    | "-Ofast"
            )
        {
            output.push(arg.clone());
            continue;
        }
        if !arg.starts_with('-') && relative(root, &command.directory, arg).as_deref() == Some(file)
        {
            continue;
        }
        return Ok(None);
    }
    Ok(Some(output))
}
