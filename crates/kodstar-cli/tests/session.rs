// Purpose: stdio 세션의 실제 요청·오류·취소·EOF 종료 계약 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test session.
// Related: crates/kodstar-cli/src/session/mod.rs, crates/kodstar-cli/tests/support/mod.rs.

mod support;
use serde_json::{Value, json};
use std::io::Write;
use std::process::Stdio;
use support::*;

fn session(project: &Project, frames: &[Value]) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    for frame in frames {
        serde_json::to_writer(&mut bytes, frame)?;
        bytes.write_all(b"\n")?;
    }
    session_raw(project, &bytes)
}

fn session_raw(project: &Project, bytes: &[u8]) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut child = project
        .command()
        .args(["serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        child
            .stdin
            .take()
            .ok_or("missing stdin")?
            .write_all(bytes)?;
    }
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

#[test]
fn sessions_return_versioned_ids_and_reject_invalid_parameters() -> TestResult {
    let project = Project::new()?;
    let output = session(
        &project,
        &[
            json!({"protocol_version":1,"id":"languages","method":"languages","params":{}}),
            json!({"protocol_version":99,"id":"bad-version","method":"languages"}),
            json!({"protocol_version":1,"id":"bad-query","method":"query","params":{"query":{"kind":"unknown"}}}),
        ],
    )?;
    assert!(output.iter().all(|event| event["protocol_version"] == 1));
    assert!(output.iter().any(|event| event["id"] == "languages"
        && event["type"] == "complete"
        && event["exit_code"] == 0));
    for id in ["bad-version", "bad-query"] {
        assert!(
            output
                .iter()
                .any(|event| event["id"] == id && event["type"] == "error" && event["code"] == 2)
        );
    }
    Ok(())
}

#[test]
fn sessions_use_the_same_index_and_can_read_without_refresh() -> TestResult {
    let project = Project::new()?;
    project.write("fn target() {} fn caller() { target(); }")?;
    assert!(project.run(&["build"])?.status.success());
    let output = session(
        &project,
        &[
            json!({"protocol_version":1,"id":1,"method":"query","params":{
                "query":{"kind":"deps","file":"main.rs","name":"caller"},"options":{"refresh":"none"}
            }}),
        ],
    )?;
    assert!(output.iter().any(|event| event["id"] == 1
        && event["type"] == "metadata"
        && event["data"]["freshness"] == "not_checked"));
    assert!(output.iter().any(|event| event["id"] == 1
        && event["type"] == "result"
        && event["data"]["target"]["name"] == "target"));
    assert!(
        output.iter().any(|event| event["id"] == 1
            && event["type"] == "complete"
            && event["exit_code"] == 0)
    );
    Ok(())
}

#[test]
fn cancellation_is_processed_while_an_analysis_request_is_active() -> TestResult {
    let project = Project::new()?;
    project.write(&format!(
        "fn target() {{}} fn caller() {{ {} }}",
        "target();".repeat(20_000)
    ))?;
    let output = session(
        &project,
        &[
            json!({"protocol_version":1,"id":"build","method":"build","params":{}}),
            json!({"protocol_version":1,"id":"cancel","method":"cancel","params":{"request_id":"build"}}),
        ],
    )?;
    assert!(output.iter().any(|event| event["id"] == "cancel"
        && event["type"] == "complete"
        && event["exit_code"] == 0));
    assert!(output.iter().any(|event| event["id"] == "build"
        && event["type"] == "complete"
        && event["exit_code"] == 130));
    Ok(())
}

#[test]
fn sessions_synchronize_diagnostics_and_support_explicit_cached_reads() -> TestResult {
    let project = Project::new()?;
    project.write("fn first() {}")?;
    assert!(project.run(&["build"])?.status.success());
    project.write("fn first() {} fn second() {}")?;
    let output = session(
        &project,
        &[
            json!({"protocol_version":1,"id":"cached","method":"status","params":{"options":{"refresh":"none"}}}),
            json!({"protocol_version":1,"id":"fresh","method":"status"}),
        ],
    )?;
    assert!(output.iter().any(|event| event["id"] == "cached"
        && event["type"] == "metadata"
        && event["data"]["generation"] == 1
        && event["data"]["freshness"] == "not_checked"));
    assert!(output.iter().any(|event| event["id"] == "fresh"
        && event["type"] == "metadata"
        && event["data"]["generation"] == 2
        && event["data"]["freshness"] == "metadata_checked"));
    Ok(())
}

#[test]
fn ambiguous_duplicate_fields_are_rejected_and_large_frames_do_not_poison_the_session() -> TestResult
{
    let project = Project::new()?;
    let duplicate = br#"{"protocol_version":1,"id":"duplicate","method":"unknown","method":"languages","params":{}}
"#;
    let output = session_raw(&project, duplicate)?;
    assert!(
        output.iter().any(|event| event["id"] == "duplicate"
            && event["type"] == "error"
            && event["code"] == 2)
    );
    let mut large = vec![b'x'; 65537];
    large.push(b'\n');
    large.extend_from_slice(
        br#"{"protocol_version":1,"id":"next","method":"languages"}
"#,
    );
    let output = session_raw(&project, &large)?;
    assert!(
        output
            .iter()
            .any(|event| event["type"] == "error" && event["code"] == 2)
    );
    assert!(output.iter().any(|event| event["id"] == "next"
        && event["type"] == "complete"
        && event["exit_code"] == 0));
    Ok(())
}
