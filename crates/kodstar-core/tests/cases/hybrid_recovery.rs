// Purpose: 기본 구문 그래프·부분 복구·선택적 native 보강의 독립성과 증분 일치 검증.
// Pattern: Integration Test.
// Usage: engine 통합 검사에서 외부 SDK나 빌드 실행 없이 검증.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/parse/mod.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;

#[test]
fn missing_sdk_keeps_a_usable_syntax_graph_and_separate_native_diagnostics() -> TestResult {
    let p = Project::new()?;
    p.write("main.cpp", "#include <missing_project_sdk.hpp>\nint target() { return 1; }\nint caller() { return target(); }\n")?;
    let report = p.build(false)?;
    assert_eq!(report.coverage.failed_files, 0);
    assert!(report.coverage.native_incomplete_files > 0);
    let rows = p.query(Query::Deps {
        selector: selector("main.cpp", "caller"),
    })?;
    assert!(rows.summary.as_ref().ok_or("summary")?.complete);
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|s| s.name == "target"))
    );
    Ok(())
}

#[test]
fn recovered_declarations_and_owners_survive_without_confirming_uncertain_calls() -> TestResult {
    let p = Project::new()?;
    p.write("main.ts", "class Service { good() {}\n broken() { const value = ; }\n after() { this.good(); }\n}\nfunction independent() {}\n")?;
    let first = p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.ts".into(),
    })?;
    for name in ["Service", "good", "after", "independent"] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == name)),
            "missing {name}"
        );
    }
    let after = rows
        .rows
        .iter()
        .find_map(|r| match r {
            QueryRow::Symbol(s) if s.name == "after" => Some(s),
            _ => None,
        })
        .ok_or("after")?;
    assert_eq!(after.qualified_name, "Service::after");
    assert!(!rows.summary.as_ref().ok_or("summary")?.complete);
    let edges = candidates(
        &p,
        Query::Related {
            selector: selector("main.ts", "after"),
        },
    )?;
    assert!(
        relationships(&edges)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|s| s.name == "good"))
    );
    assert!(
        relationships(&edges)
            .iter()
            .all(|r| r.resolution != "resolved")
    );
    assert_eq!(
        normalized_index(&first)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn syntax_recovery_changes_invalidate_consumers_even_if_their_facts_did_not_change() -> TestResult {
    let p = Project::new()?;
    let clean = "export function target() {}\n";
    p.write("api.ts", clean)?;
    p.write(
        "caller.ts",
        "import { target } from './api'; export function caller() { target(); }",
    )?;
    p.build(false)?;
    p.write("api.ts", &format!("{clean}const broken = ;\n"))?;
    let partial = p.build(false)?;
    let rows = candidates(
        &p,
        Query::Explain {
            selector: selector("caller.ts", "caller"),
        },
    )?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|s| s.name == "target"))
    );
    assert!(
        relationships(&rows)
            .iter()
            .filter(|r| r.target.as_ref().is_some_and(|s| s.name == "target"))
            .all(|r| r.resolution != "resolved")
    );
    assert_eq!(
        normalized_index(&partial)?,
        normalized_index(&p.build(true)?)?
    );
    p.write("api.ts", clean)?;
    let restored = p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("caller.ts", "caller"),
    })?;
    assert!(relationships(&rows).iter().any(|r| {
        r.target.as_ref().is_some_and(|s| s.name == "target") && r.resolution == "resolved"
    }));
    assert_eq!(
        normalized_index(&restored)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn recovered_named_modules_keep_their_actual_qualified_owners() -> TestResult {
    for (file, source, expected) in [
        (
            "main.cs",
            "namespace Demo { class Service { public void Good() {} public void Broken() { var x = ; } public void After() {} } }",
            "Demo::Service::After",
        ),
        (
            "main.rs",
            "mod demo { pub fn good() {} fn broken() { let value = ; } pub fn after() {} }",
            "demo::after",
        ),
        (
            "main.ex",
            "defmodule Demo do\n def good, do: :ok\n def broken, do: =\n def after_error, do: :ok\nend\n",
            "Demo::good",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        let report = p.build(false)?;
        assert_eq!(report.coverage.partial_files, 1, "{file}");
        let rows = p.query(Query::Symbols { file: file.into() })?;
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r, QueryRow::Symbol(s) if s.qualified_name == expected)),
            "missing {expected}: {:?}",
            rows.rows
        );
        assert_eq!(
            normalized_index(&report)?,
            normalized_index(&p.build(true)?)?
        );
    }
    Ok(())
}

fn candidates(
    p: &Project,
    query: Query,
) -> std::result::Result<Collected, Box<dyn std::error::Error>> {
    let mut sink = Collected::default();
    p.engine.query(
        &p.options(),
        &query,
        &QueryOptions {
            refresh: Refresh::None,
            include_candidates: true,
            limit: Limit::Unlimited,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut sink,
    )?;
    Ok(sink)
}

#[test]
fn embedded_recovery_reports_original_locations_and_keeps_script_boundaries() -> TestResult {
    for file in ["Main.ejs", "Main.astro"] {
        let p = Project::new()?;
        let source = "<h1>한글</h1>\r\n<script>function broken(){</script>\r\n<script>function intact(){}</script>";
        p.write(file, source)?;
        let report = p.build(false)?;
        assert_eq!(report.coverage.partial_files, 1);
        let rows = p.query(Query::Symbols { file: file.into() })?;
        let intact = rows
            .rows
            .iter()
            .find_map(|r| match r {
                QueryRow::Symbol(s) if s.name == "intact" => Some(s),
                _ => None,
            })
            .ok_or("independent script definition")?;
        assert_eq!(intact.definition.line, 3);
        assert!(
            source[intact.definition.start_byte..intact.definition.end_byte]
                .contains("function intact")
        );
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
        assert!(diagnostics.rows.iter().any(|r| matches!(r, QueryRow::Diagnostic(d) if d.code == "source_syntax_partial" && d.line == Some(2))));
    }
    Ok(())
}

#[test]
fn ordinary_swift_strings_reject_unescaped_newlines_without_losing_earlier_declarations()
-> TestResult {
    let p = Project::new()?;
    p.write(
        "text.swift",
        "func before() {}\nlet text = \"line\nbreak\"\n",
    )?;
    let report = p.build(false)?;
    assert_eq!(report.coverage.partial_files, 1);
    let rows = p.query(Query::Symbols {
        file: "text.swift".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "before"))
    );
    Ok(())
}
