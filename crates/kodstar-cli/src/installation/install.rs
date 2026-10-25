// Purpose: CLI·데이터 홈의 사용자 설치와 중단 후 재시도 가능한 교체 흐름.
// Pattern: Use Case.
// Usage: install.sh의 내부 __install 요청을 실행.
// Related: crates/kodstar-cli/src/installation/mod.rs, crates/kodstar-cli/src/installation/files.rs, crates/kodstar-cli/src/installation/shell.rs.

use super::{Receipt, files, home, receipt, save, shell, validate};
use files::failure;
use kodstar_core::{Error, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    process::ExitCode,
};

pub(crate) fn run() -> Result<ExitCode> {
    let home = home()?;
    let source =
        std::env::current_exe().map_err(|e| Error::caused_by("locate installation source", e))?;
    let mut created = Vec::new();
    files::directory(&home.join(".kodstar"), &mut created)?;
    #[cfg(unix)]
    if created.contains(&home.join(".kodstar")) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home.join(".kodstar"), fs::Permissions::from_mode(0o700))
            .map_err(|e| Error::caused_by("protect application home", e))?;
    }
    let lock_path = home.join(".kodstar/installation.lock");
    files::read(&lock_path)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| Error::caused_by("open installation lock", e))?;
    lock.try_lock().map_err(|e| {
        Error::caused_by(
            "KodStar is in use; stop running sessions before updating",
            e,
        )
    })?;
    let existing = receipt(&home)?;
    if let Some(receipt) = &existing {
        validate(receipt)?;
    }
    let binary = home.join(".local/bin/kodstar");
    if existing.is_none() && fs::symlink_metadata(&binary).is_ok() {
        files::regular(&binary)?;
        // The explicit installer invocation authorizes replacing the existing KodStar binary.
    }
    files::plan_directory(
        binary
            .parent()
            .ok_or_else(|| failure("missing binary directory"))?,
        &mut created,
    )?;
    let mut receipt = match existing {
        Some(mut receipt) => {
            receipt.previous_digest = if binary.exists() {
                Some(files::digest(&binary)?)
            } else {
                None
            };
            receipt.digest = files::digest(&source)?;
            receipt
        }
        None => Receipt {
            version: 1,
            binary: binary.clone(),
            digest: files::digest(&source)?,
            previous_digest: if binary.exists() {
                Some(files::digest(&binary)?)
            } else {
                None
            },
            shell: shell::plan(
                &home,
                binary
                    .parent()
                    .ok_or_else(|| failure("missing binary directory"))?,
            )?,
            directories: Vec::new(),
        },
    };
    for edit in &receipt.shell {
        files::plan_directory(
            edit.path
                .parent()
                .ok_or_else(|| failure("missing shell directory"))?,
            &mut created,
        )?;
    }
    receipt
        .directories
        .extend(created.into_iter().filter(|p| p != &home.join(".kodstar")));
    receipt.directories.sort_by_key(|p| p.components().count());
    receipt.directories.dedup();
    // Publish ownership first so interrupted installs can be retried or uninstalled.
    if receipt.directories.iter().any(|p| !p.starts_with(&home)) {
        return Err(failure(
            "create an external ZDOTDIR directory before installing",
        ));
    }
    save(&home, &receipt)?;
    for directory in &receipt.directories {
        files::directory(directory, &mut Vec::new())?;
    }
    for edit in &receipt.shell {
        if shell::validate(edit)?.is_none() {
            shell::apply(edit)?;
        }
    }
    files::atomic(
        &binary,
        &mut |out| {
            let mut input =
                File::open(&source).map_err(|e| Error::caused_by("open built binary", e))?;
            std::io::copy(&mut input, out).map_err(|e| Error::caused_by("copy built binary", e))?;
            Ok(())
        },
        0o755,
    )?;
    receipt.previous_digest = None;
    save(&home, &receipt)?;
    let mut stdout = std::io::stdout().lock();
    writeln!(
        stdout,
        "Installed kodstar {}\n  Executable: {}\n  Data home:  {}",
        env!("CARGO_PKG_VERSION"),
        binary.display(),
        home.join(".kodstar").display()
    )
    .map_err(|e| Error::caused_by("write installation result", e))?;
    let bin = shell::quote(
        binary
            .parent()
            .ok_or_else(|| failure("missing binary directory"))?,
    )?;
    let activate = if std::env::var_os("SHELL").is_some_and(|s| {
        std::path::Path::new(&s)
            .file_name()
            .is_some_and(|n| n == "fish")
    }) {
        format!("set -gx PATH {bin} $PATH")
    } else {
        format!("export PATH={bin}:\"$PATH\"")
    };
    writeln!(
        stdout,
        "\nNew terminals recognize kodstar automatically. For this terminal, run:\n{activate}\n\nGet started: kodstar init /path/to/project"
    )
    .map_err(|e| Error::caused_by("write PATH activation", e))?;
    Ok(ExitCode::SUCCESS)
}
