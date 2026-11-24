// Purpose: 단계·현재 작업·native 진단을 stderr에 제한된 빈도로 표시.
// Pattern: Output Adapter.
// Usage: CLI sink가 terminal·pipe·redirect 모두의 진행 이벤트를 전달.
// Related: crates/kodstar-cli/src/output.rs, crates/kodstar-cli/src/run.rs.

use kodstar_core::{Error, Progress, Result};
use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

pub(crate) struct ProgressPrinter {
    stderr: io::Stderr,
    clock: Instant,
    state: Display,
    verbose: bool,
}
impl ProgressPrinter {
    pub(crate) fn new(verbose: bool) -> Self {
        Self {
            stderr: io::stderr(),
            clock: Instant::now(),
            state: Display::default(),
            verbose,
        }
    }
    pub(crate) fn update(&mut self, progress: &Progress) -> Result<()> {
        if let Some(line) = self
            .state
            .line(progress, self.clock.elapsed(), self.verbose)
        {
            writeln!(self.stderr, "{line}").map_err(|e| Error::caused_by("write progress", e))?;
        }
        Ok(())
    }
}
#[derive(Default)]
struct Display {
    phase: Option<&'static str>,
    last: Duration,
    native_seen: bool,
}
impl Display {
    fn line(&mut self, event: &Progress, now: Duration, verbose: bool) -> Option<String> {
        let warning = matches!(
            event.phase,
            "native_limited"
                | "native_incomplete"
                | "input_failed"
                | "configuration_incomplete"
                | "syntax_partial"
        );
        if !verbose && !warning {
            return None;
        }
        if event.phase == "native_wait" {
            return None;
        }
        if event.phase == "native" {
            if self.native_seen && now.saturating_sub(self.last) < Duration::from_secs(1) {
                return None;
            }
            self.native_seen = true;
        } else if !warning {
            if self.phase == Some(event.phase)
                && now.saturating_sub(self.last) < Duration::from_secs(2)
            {
                return None;
            }
            self.phase = Some(event.phase);
        }
        self.last = now;
        let label = match event.phase {
            "lock" => "Waiting for repository lock",
            "capture" => "Capturing source changes",
            "parse" => "Parsing and analyzing",
            "configuration" => "Reading referenced configuration",
            "native" => "Clang semantic analysis",
            "native_limited" => "Clang task limited",
            "native_incomplete" => "Clang context incomplete",
            "input_failed" => "Source analysis failed",
            "syntax_partial" => "Tree-sitter partial analysis",
            "configuration_incomplete" => "Configuration context incomplete",
            "resolve_prepare" => "Preparing name and type lookup",
            "project_links" => "Linking project metadata",
            "resolve" => "Resolving relationships",
            "dispatch" => "Connecting dispatch candidates",
            "publish" => "Publishing index",
            "execute" => "Executing requested operation",
            _ => event.phase,
        };
        let mut line = format!("kodstar: {label}");
        if let Some(task) = &event.current_task {
            line.push_str(&format!(" | {}", clean(task)))
        }
        if event.completed_files > 0 {
            line.push_str(&format!(" | {} files", event.completed_files))
        }
        if let Some(detail) = &event.detail {
            line.push_str(&format!(" | {}", clean(detail)))
        }
        Some(line)
    }
}
fn clean(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() {
            clean.extend(ch.escape_default());
        } else {
            clean.push(ch);
        }
    }
    clean
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phases_are_visible_without_a_terminal_and_file_updates_are_throttled() {
        let mut output = Display::default();
        let start = output
            .line(&Progress::phase("capture"), Duration::ZERO, true)
            .unwrap();
        assert!(start.contains("Capturing source changes"));
        assert!(
            output
                .line(
                    &Progress::phase("capture"),
                    Duration::from_millis(100),
                    true
                )
                .is_none()
        );
        assert!(
            output
                .line(
                    &Progress::phase("resolve"),
                    Duration::from_millis(200),
                    true
                )
                .unwrap()
                .contains("Resolving relationships")
        );
        let task = Progress::task("native", "main.cpp".into(), None);
        assert!(
            output
                .line(&task, Duration::from_millis(250), true)
                .unwrap()
                .contains("main.cpp")
        );
        assert!(
            output
                .line(&task, Duration::from_millis(300), true)
                .is_none()
        );
    }
    #[test]
    fn warnings_are_never_throttled_and_control_characters_are_escaped() {
        let mut output = Display::default();
        assert!(
            output
                .line(&Progress::phase("capture"), Duration::ZERO, false)
                .is_none()
        );
        assert!(
            output
                .line(&Progress::phase("native"), Duration::ZERO, false)
                .is_none()
        );
        let event = Progress::task(
            "native_limited",
            "file\n.cpp".into(),
            Some("tokens: used 10, limit 9".into()),
        );
        for verbose in [true, false] {
            let line = output.line(&event, Duration::ZERO, verbose).unwrap();
            assert!(line.contains("file\\n.cpp"));
            assert!(line.contains("used 10"));
            assert!(!line.contains('\n'));
        }
    }
}
