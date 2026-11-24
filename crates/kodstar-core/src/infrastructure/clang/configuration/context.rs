// Purpose: 저장된 컴파일 문맥과 실제 include 근거로 헤더의 분석 소유자 선택.
// Pattern: Boundary Adapter.
// Usage: native session이 고정 snapshot 입력에서 TU와 flags를 요청.
// Related: crates/kodstar-core/src/infrastructure/clang/configuration.rs, crates/kodstar-core/src/infrastructure/clang/configuration/database.rs.

use super::{Database, ProjectInputs};
use crate::domain::{diagnostics::Result, native::Observation};
use std::path::{Path, PathBuf};

pub(in crate::infrastructure::clang) struct Context {
    pub arguments: Option<Vec<String>>,
    pub owner: Option<(String, Vec<u8>)>,
}

pub(in crate::infrastructure::clang) fn prepare(
    input: &mut ProjectInputs,
    root: &Path,
    file: &Path,
    database: &mut Option<Database>,
) -> Result<Context> {
    let mut arguments = super::arguments(input, root, file, database)?;
    if arguments.is_none() {
        return Ok(Context {
            arguments,
            owner: None,
        });
    }
    let mut roots = vec![PathBuf::new(), PathBuf::from("include")];
    for directory in ["src", "lib"] {
        input.entry(directory, false)?;
        if matches!(
            input.dependencies.get(directory),
            Some(Observation::Directory)
        ) {
            roots.push(directory.into());
        }
    }
    let header = file
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| matches!(s, "h" | "hpp" | "hxx" | "hh" | "inl"));
    let selected = if header && database.as_ref().is_none_or(|d| d.select(file).is_none()) {
        database.as_ref().and_then(|d| d.owner(file))
    } else {
        None
    };
    if matches!(selected, Some(None)) {
        return Ok(Context {
            arguments: None,
            owner: None,
        });
    }
    let candidate = if header {
        selected.flatten().or_else(|| {
            let stem = file.file_stem()?.to_str()?.strip_suffix("-inl")?;
            Some(file.with_file_name(format!("{stem}.{}", file.extension()?.to_str()?)))
        })
    } else {
        None
    };
    let mut owner = None;
    if let Some(candidate) = candidate
        && let Some(path) = candidate.to_str()
        && let Some((_, Some(bytes))) = input.entry(path, true)?
    {
        let selected = super::arguments(input, root, &candidate, database)?;
        if let Some(flags) = selected.as_ref() {
            let mut include_roots = include_arguments(flags);
            include_roots.extend(roots.clone());
            if includes(input, &candidate, &bytes, file, &include_roots)? {
                arguments = selected;
                owner = Some((format!("/__kodstar_project/{path}"), bytes));
            }
        }
    }
    if let Some(arguments) = &mut arguments {
        for path in roots {
            arguments.push(format!("-I/__kodstar_project/{}", path.display()));
        }
    }
    Ok(Context { arguments, owner })
}

fn include_arguments(arguments: &[String]) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut at = 0;
    while let Some(flag) = arguments.get(at) {
        let value = if matches!(flag.as_str(), "-I" | "-iquote" | "-isystem") {
            at += 1;
            arguments.get(at).map(String::as_str)
        } else {
            flag.strip_prefix("-I")
        };
        if let Some(value) = value.and_then(|v| v.strip_prefix("/__kodstar_project/")) {
            result.push(PathBuf::from(value));
        }
        at += 1;
    }
    result
}

fn includes(
    input: &mut ProjectInputs,
    owner: &Path,
    source: &[u8],
    target: &Path,
    roots: &[PathBuf],
) -> Result<bool> {
    for line in source.split(|&b| b == b'\n') {
        input.cancel.check()?;
        let Some(line) = line.trim_ascii_start().strip_prefix(b"#") else {
            continue;
        };
        let Some(line) = line.trim_ascii_start().strip_prefix(b"include") else {
            continue;
        };
        let line = line.trim_ascii_start();
        let quote = match line.first() {
            Some(b'"') => b'"',
            Some(b'<') => b'>',
            _ => continue,
        };
        let Some(end) = line[1..].iter().position(|&b| b == quote) else {
            continue;
        };
        let Ok(name) = std::str::from_utf8(&line[1..end + 1]) else {
            continue;
        };
        let relative = if quote == b'"' { owner.parent() } else { None };
        for base in relative
            .into_iter()
            .chain(roots.iter().map(PathBuf::as_path))
        {
            let Some(candidate) = super::relative(
                Path::new("/__kodstar_project"),
                &base.to_string_lossy(),
                name,
            ) else {
                continue;
            };
            if candidate == target {
                return Ok(true);
            }
            if let Some(path) = candidate.to_str()
                && input.entry(path, false)?.is_some()
            {
                break;
            }
        }
    }
    Ok(false)
}
