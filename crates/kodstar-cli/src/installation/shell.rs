// Purpose: 사용자 shell의 소유한 PATH 블록 등록·제거.
// Pattern: Filesystem Adapter.
// Usage: 설치 receipt가 변경할 파일과 삽입 내용을 기록.
// Related: crates/kodstar-cli/src/installation/mod.rs, crates/kodstar-cli/src/installation/files.rs.

use super::files::{self, failure};
use kodstar_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const START: &str = "# >>> kodstar PATH >>>";
const END: &str = "# <<< kodstar PATH <<<";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Edit {
    pub entry: PathBuf,
    pub path: PathBuf,
    pub addition: String,
    pub created: bool,
    pub mode: u32,
}

pub(super) fn quote(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| failure("installation paths must be UTF-8"))?;
    if value.chars().any(|c| c.is_control()) {
        return Err(failure(
            "installation paths cannot contain control characters",
        ));
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

pub(super) fn plan(home: &Path, bin: &Path) -> Result<Vec<Edit>> {
    let quoted = quote(bin)?;
    let body = format!(
        "{START}\ncase \"${{PATH-}}\" in\n  {quoted}|{quoted}:*) ;;\n  *) export PATH={quoted}:\"${{PATH-}}\" ;;\nesac\n{END}\n"
    );
    let zsh = std::env::var_os("ZDOTDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.to_path_buf());
    if !zsh.is_absolute() {
        return Err(failure("ZDOTDIR must be absolute"));
    }
    let zsh = if zsh
        .try_exists()
        .map_err(|e| Error::caused_by("inspect ZDOTDIR", e))?
    {
        fs::canonicalize(&zsh).map_err(|e| Error::caused_by("resolve ZDOTDIR", e))?
    } else {
        zsh
    };
    let mut paths = vec![
        home.join(".profile"),
        home.join(".bashrc"),
        zsh.join(".zshrc"),
    ];
    // Creating .bash_profile would suppress a user's existing .profile login behavior.
    for name in [".bash_profile", ".bash_login"] {
        if home.join(name).exists() {
            paths.push(home.join(name));
            break;
        }
    }
    let fish = home.join(".config/fish/config.fish");
    let uses_fish = std::env::var_os("SHELL")
        .is_some_and(|s| Path::new(&s).file_name().is_some_and(|n| n == "fish"));
    if uses_fish || fish.exists() {
        paths.push(fish.clone());
    }
    let fish_body = format!(
        "{START}\nif test \"$PATH[1]\" != {quoted}\n  set -gx PATH {quoted} $PATH\nend\n{END}\n"
    );
    paths
        .into_iter()
        .map(|entry| {
            let content = if entry == fish { &fish_body } else { &body };
            prepare(entry, content)
        })
        .collect()
}

fn prepare(entry: PathBuf, content: &str) -> Result<Edit> {
    let path = if fs::symlink_metadata(&entry).is_ok() {
        fs::canonicalize(&entry).map_err(|e| Error::caused_by("resolve shell configuration", e))?
    } else {
        entry.clone()
    };
    let existing = files::read(&path)?;
    let created = existing.is_none();
    let text = String::from_utf8(existing.unwrap_or_default())
        .map_err(|e| Error::caused_by("shell configuration must be UTF-8", e))?;
    if text.contains(START) || text.contains(END) {
        return Err(failure(format!(
            "unregistered KodStar PATH block; inspect {}",
            path.display()
        )));
    }
    let separator = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let addition = format!("{separator}{content}");
    #[cfg(unix)]
    let mode = if created {
        0o644
    } else {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&path)
            .map_err(|e| Error::caused_by("inspect shell permissions", e))?
            .permissions()
            .mode()
            & 0o777
    };
    Ok(Edit {
        entry,
        path,
        addition,
        created,
        mode,
    })
}

pub(super) fn validate(edit: &Edit) -> Result<Option<String>> {
    if edit.entry != edit.path {
        let actual = fs::canonicalize(&edit.entry)
            .map_err(|e| Error::caused_by("resolve registered shell link", e))?;
        if actual != edit.path {
            return Err(failure("registered shell link changed; data was preserved"));
        }
    }
    let Some(bytes) = files::read(&edit.path)? else {
        return Ok(None);
    };
    let text =
        String::from_utf8(bytes).map_err(|e| Error::caused_by("read shell configuration", e))?;
    if !text.contains(START) && !text.contains(END) {
        return Ok(None);
    }
    if text.matches(&edit.addition).count() != 1
        || text.matches(START).count() != 1
        || text.matches(END).count() != 1
    {
        return Err(failure(format!(
            "KodStar PATH block changed; preserved {}",
            edit.path.display()
        )));
    }
    Ok(Some(text.replacen(&edit.addition, "", 1)))
}

pub(super) fn apply(edit: &Edit) -> Result<()> {
    let mut text = files::read(&edit.path)?.unwrap_or_default();
    text.extend_from_slice(edit.addition.as_bytes());
    files::write(&edit.path, &text, edit.mode)
}

pub(super) fn remove(edit: &Edit) -> Result<()> {
    if let Some(text) = validate(edit)? {
        if edit.created && text.is_empty() {
            fs::remove_file(&edit.path)
                .map_err(|e| Error::caused_by("remove owned shell file", e))?;
        } else {
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                fs::metadata(&edit.path)
                    .map_err(|e| Error::caused_by("inspect shell permissions", e))?
                    .permissions()
                    .mode()
                    & 0o777
            };
            files::write(&edit.path, text.as_bytes(), mode)?;
        }
    }
    Ok(())
}
