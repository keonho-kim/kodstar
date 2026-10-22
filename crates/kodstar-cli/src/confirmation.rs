// Purpose: 명시적 경로와 파괴적 제거에 대한 터미널 확인.
// Pattern: Presentation Boundary.
// Usage: run이 최초 init 또는 uninstall 전에 호출.
// Related: crates/kodstar-cli/src/run.rs, crates/kodstar-cli/src/installation/mod.rs.

use kodstar_core::{Error, Result};
use std::io::{self, BufRead, IsTerminal, Read, Write};

pub(crate) fn ask(description: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(Error::argument(format!(
            "{description}\nconfirmation requires a terminal; review the target and pass --yes for non-interactive use"
        )));
    }
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{description}")
        .and_then(|()| write!(stderr, "Continue? [y/N] "))
        .and_then(|()| stderr.flush())
        .map_err(|e| Error::caused_by("write confirmation", e))?;
    let mut answer = String::new();
    // Bound terminal input without retaining an arbitrary pasted source document.
    io::stdin()
        .lock()
        .take(32)
        .read_line(&mut answer)
        .map_err(|e| Error::caused_by("read confirmation", e))?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        return Err(Error::argument("cancelled; no changes were made"));
    }
    Ok(())
}
