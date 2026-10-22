// Purpose: 실제 watch 프로세스의 SIGINT·완료 이벤트·종료 코드 검증.
// Pattern: Integration Test.
// Usage: Unix에서 cargo test -p kodstar-cli --test watch.
// Related: crates/kodstar-cli/src/signals.rs, crates/kodstar-cli/tests/support/mod.rs.

#[cfg(unix)]
mod support;

#[cfg(unix)]
#[test]
fn sigint_finishes_watch_and_returns_130() -> support::TestResult {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    use std::io::{BufRead, Read};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Duration;
    let project = support::Project::new()?;
    project.write("fn target() {}")?;
    assert!(project.run(&["init"])?.status.success());
    let mut child = project
        .command()
        .args(["watch", "--format", "jsonl"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = Pid::from_raw(i32::try_from(child.id())?);
    let result = std::thread::scope(|scope| -> support::TestResult {
        let (done, timeout) = mpsc::channel();
        scope.spawn(move || {
            if timeout.recv_timeout(Duration::from_secs(10)).is_err() {
                let _ = kill(pid, Signal::SIGKILL);
            }
        });
        let result = (|| -> support::TestResult {
            let mut reader =
                std::io::BufReader::new(child.stdout.take().ok_or("missing watch output")?);
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                return Err("watch did not emit metadata".into());
            }
            let event: serde_json::Value = serde_json::from_str(&line)?;
            assert_eq!(event["type"], "metadata");
            line.clear();
            reader.read_line(&mut line)?;
            kill(pid, Signal::SIGINT)?;
            let status = child.wait()?;
            let mut rest = String::new();
            reader.read_to_string(&mut rest)?;
            let last = rest.lines().last().ok_or("missing watch summary")?;
            let summary: serde_json::Value = serde_json::from_str(last)?;
            assert_eq!(summary["type"], "summary");
            assert_eq!(summary["complete"], false);
            assert_eq!(status.code(), Some(130));
            Ok(())
        })();
        let _ = done.send(());
        result
    });
    let _ = child.kill();
    let _ = child.wait();
    result
}
