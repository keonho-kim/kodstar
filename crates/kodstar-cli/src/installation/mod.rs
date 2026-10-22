// Purpose: 설치 receipt 검증과 설치 흔적의 완전 제거.
// Pattern: Use Case.
// Usage: install.sh의 내부 설치 요청과 kodstar uninstall이 호출.
// Related: install.sh, crates/kodstar-cli/src/installation/files.rs, crates/kodstar-cli/src/installation/shell.rs.

mod files;
mod install;
pub(crate) use install::run as install;
mod shell;
use files::failure;
use kodstar_core::{Cancellation, Engine, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    binary: PathBuf,
    digest: String,
    previous_digest: Option<String>,
    shell: Vec<shell::Edit>,
    directories: Vec<PathBuf>,
}

fn home() -> Result<PathBuf> {
    let value = std::env::var_os("HOME").ok_or_else(|| failure("HOME is unavailable"))?;
    let home = PathBuf::from(value);
    if !home.is_absolute() || home.parent().is_none() {
        return Err(failure("HOME must be an absolute user directory"));
    }
    fs::canonicalize(&home).map_err(|e| Error::caused_by("resolve user home", e))
}

fn receipt(home: &Path) -> Result<Option<Receipt>> {
    let path = home.join(".kodstar/install.json");
    let Some(bytes) = files::read(&path)? else {
        return Ok(None);
    };
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|_| failure("invalid installation receipt; data was preserved"))?;
    if receipt.version != 1
        || receipt.binary != home.join(".local/bin/kodstar")
        || receipt.shell.len() > 8
        || receipt.directories.len() > 16
    {
        return Err(failure(
            "unsupported installation receipt; data was preserved",
        ));
    }
    for edit in &receipt.shell {
        if !edit.path.is_absolute()
            || !edit.entry.is_absolute()
            || !matches!(
                edit.entry.file_name().and_then(|n| n.to_str()),
                Some(
                    ".profile"
                        | ".bashrc"
                        | ".bash_profile"
                        | ".bash_login"
                        | ".zshrc"
                        | "config.fish"
                )
            )
        {
            return Err(failure("invalid shell path in installation receipt"));
        }
    }
    if receipt.directories.iter().any(|p| {
        !p.starts_with(home)
            || p == home
            || p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
    }) {
        return Err(failure("invalid owned directory in installation receipt"));
    }
    Ok(Some(receipt))
}

fn validate(receipt: &Receipt) -> Result<()> {
    for directory in &receipt.directories {
        files::plan_directory(directory, &mut Vec::new())?;
    }
    files::plan_directory(
        receipt
            .binary
            .parent()
            .ok_or_else(|| failure("missing binary directory"))?,
        &mut Vec::new(),
    )?;
    if fs::symlink_metadata(&receipt.binary).is_ok() {
        let hash = files::digest(&receipt.binary)?;
        if hash != receipt.digest && receipt.previous_digest.as_ref() != Some(&hash) {
            return Err(failure(format!(
                "installed binary changed; preserved {}",
                receipt.binary.display()
            )));
        }
    }
    for edit in &receipt.shell {
        shell::validate(edit)?;
    }
    Ok(())
}

fn save(home: &Path, receipt: &Receipt) -> Result<()> {
    let bytes = serde_json::to_vec(receipt)
        .map_err(|e| Error::caused_by("encode installation receipt", e))?;
    files::write(&home.join(".kodstar/install.json"), &bytes, 0o600)
}

pub(crate) fn uninstall(yes: bool) -> Result<ExitCode> {
    let home = home()?;
    let receipt = receipt(&home)?;
    if let Some(receipt) = &receipt {
        validate(receipt)?;
    }
    crate::confirmation::ask(
        &format!(
            "Remove KodStar installation, managed PATH blocks, settings, caches, and ALL registered project checkpoint histories for {}? Original source files will remain.",
            home.display()
        ),
        yes,
    )?;
    let engine = Engine::new(home.join(".kodstar"))?;
    let removal = engine.remove_local_data(&Cancellation::default())?;
    if let Some(receipt) = receipt {
        validate(&receipt)?;
        for edit in &receipt.shell {
            shell::remove(edit)?;
        }
        if receipt.binary.exists() {
            fs::remove_file(&receipt.binary)
                .map_err(|e| Error::caused_by("remove installed binary", e))?;
        }
        fs::remove_file(home.join(".kodstar/install.json"))
            .map_err(|e| Error::caused_by("remove installation receipt", e))?;
        for path in receipt.directories.iter().rev() {
            match fs::remove_dir(path) {
                Ok(()) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(e) => return Err(Error::caused_by("remove empty installation directory", e)),
            }
        }
    }
    removal.finish()?;
    writeln!(
        std::io::stdout(),
        "KodStar uninstalled. Original source files were preserved."
    )
    .map_err(|e| Error::caused_by("write uninstall result", e))?;
    Ok(ExitCode::SUCCESS)
}
