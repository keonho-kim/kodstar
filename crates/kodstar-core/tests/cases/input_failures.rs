// Purpose: 입력 실패 격리·복구와 인코딩 원본 위치 계약 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 작은 임시 프로젝트로 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/application/analysis/workers.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn failed_file_is_visible_while_valid_results_and_history_remain_usable() -> TestResult {
    let p = Project::new()?;
    p.write("valid.rs", "fn target() {} fn caller() { target(); }")?;
    p.write("broken.php", "<?php ); // intentionally invalid fixture")?;
    let report = p.build(false)?;
    assert_eq!(report.coverage.partial_files, 1);
    assert_eq!(report.coverage.analyzed_files, 2);
    let rows = p.query(Query::Deps {
        selector: selector("valid.rs", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|t| t.name == "target"))
    );
    let summary = rows.summary.ok_or("summary")?;
    assert!(!summary.complete);
    assert!(summary.reasons.contains(&"source_syntax_partial"));
    let mut diagnostics = Collected::default();
    p.engine.execute(
        &p.options(),
        Command::Status {
            doctor: true,
            refresh: Refresh::None,
        },
        &Cancellation::default(),
        &mut diagnostics,
    )?;
    assert!(diagnostics.rows.iter().any(|row| matches!(row, QueryRow::Diagnostic(d)
        if d.code == "source_syntax_partial" && d.file.as_deref() == Some("broken.php") && d.line.is_some())));
    p.write("broken.php", "<?php function recovered() {}")?;
    let recovered = p.build(false)?;
    assert_eq!(recovered.coverage.failed_files, 0);
    assert!(recovered.history.is_some());
    let full = p.build(true)?;
    assert_eq!(recovered.coverage.symbols, full.coverage.symbols);
    Ok(())
}

#[test]
fn failed_provider_does_not_leave_a_stale_resolved_target() -> TestResult {
    let p = Project::new()?;
    p.write("provider.py", "def target(): pass")?;
    p.write(
        "caller.py",
        "from provider import target\ndef caller(): target()",
    )?;
    p.build(false)?;
    p.write("provider.py", "def target( { ")?;
    let partial = p.build(false)?;
    assert_eq!(partial.coverage.partial_files, 1);
    let rows = p.query(Query::Explain {
        selector: selector("caller.py", "caller"),
    })?;
    assert!(!relationships(&rows).iter().any(
        |r| r.resolution == "resolved" && r.target.as_ref().is_some_and(|t| t.name == "target")
    ));
    p.write("provider.py", "def target(): pass")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("caller.py", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|t| t.name == "target"))
    );
    Ok(())
}

#[test]
fn declared_encoding_preserves_original_byte_ranges() -> TestResult {
    let p = Project::new()?;
    let bytes =
        b"# coding: iso-8859-5\n# \xe2\xd5\xe1\xe2\ndef target(): pass\ndef caller(): target()\n";
    std::fs::write(p.root.join("main.py"), bytes)?;
    let report = p.build(false)?;
    assert_eq!(report.coverage.failed_files, 0);
    let rows = p.query(Query::Deps {
        selector: selector("main.py", "caller"),
    })?;
    let edges = relationships(&rows);
    let call = edges
        .iter()
        .find(|r| r.target.as_ref().is_some_and(|s| s.name == "target"))
        .ok_or("encoded call")?;
    assert_eq!(
        &bytes[call.site.span.start_byte..call.site.span.end_byte],
        b"target"
    );
    assert_eq!(call.site.span.line, 4);
    assert_eq!(std::fs::read(p.root.join("main.py"))?, bytes);
    Ok(())
}

#[test]
fn named_class_expression_keeps_members_and_export_without_leaking_its_name() -> TestResult {
    let p = Project::new()?;
    p.write("app.js", "module.exports = class Application { handle() { return Application; } }; function outside() { return Application; }")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "app.js".into(),
    })?;
    for name in ["Application", "Application::handle"] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r, QueryRow::Symbol(s) if s.qualified_name == name)),
            "missing {name}"
        );
    }
    let rows = p.query(Query::Explain {
        selector: selector("app.js", "outside"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|r| r.resolution == "resolved"
                && r.target.as_ref().is_some_and(|t| t.name == "Application"))
    );
    Ok(())
}

#[test]
fn failed_input_selection_is_unavailable_instead_of_an_authoritative_absence() -> TestResult {
    let p = Project::new()?;
    p.write("main.rs", "fn broken( {")?;
    p.build(false)?;
    let mut sink = Collected::default();
    let error = p
        .engine
        .query(
            &p.options(),
            &Query::Explain {
                selector: selector("main.rs", "broken"),
            },
            &QueryOptions {
                refresh: Refresh::None,
                ..QueryOptions::default()
            },
            &Cancellation::default(),
            &mut sink,
        )
        .expect_err("unavailable source");
    assert_eq!(error.kind, ErrorKind::Limit);
    assert!(
        sink.summary
            .ok_or("summary")?
            .reasons
            .contains(&"source_unavailable")
    );
    Ok(())
}

#[test]
fn doctor_lists_invalid_configuration_while_preserving_valid_source_results() -> TestResult {
    let p = Project::new()?;
    p.write("main.ts", "export function valid() {}")?;
    p.write("tsconfig.json", "{ invalid configuration")?;
    let report = p.build(false)?;
    assert_eq!(report.coverage.failed_files, 1);
    assert_eq!(report.coverage.configuration_incomplete_files, 1);
    let mut sink = Collected::default();
    p.engine.execute(
        &p.options(),
        Command::Status {
            doctor: true,
            refresh: Refresh::None,
        },
        &Cancellation::default(),
        &mut sink,
    )?;
    assert!(
        sink.rows.iter().any(
            |r| matches!(r, QueryRow::Status(s) if s.generation > 0 && s.last_error.is_none())
        )
    );
    assert!(sink.rows.iter().any(|r| matches!(r, QueryRow::Diagnostic(d) if d.code == "configuration_invalid" && d.file.as_deref() == Some("tsconfig.json"))));
    assert!(!sink.summary.ok_or("summary")?.complete);
    Ok(())
}
