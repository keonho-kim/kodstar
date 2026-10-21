// Purpose: EJS module 격리·classic 공유·독립 script 문법과 원본 위치 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/src/infrastructure/formats/components/ejs/mod.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn ejs_module_private_functions_do_not_leak_to_sibling_modules_or_classic_scripts() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs", "<script type=\"module\">function privateOnly(){} function own(){privateOnly();}</script>\n<script type=\"module\">function sibling(){privateOnly();}</script>\n<script>function classic(){privateOnly();}</script>")?;
    p.build(false)?;
    for caller in ["own", "sibling", "classic"] {
        let rows = p.query(Query::Explain {
            selector: selector("Main.ejs", caller),
        })?;
        let linked = relationships(&rows).iter().any(|e| {
            e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == "privateOnly")
        });
        assert_eq!(linked, caller == "own", "{caller}: {:?}", rows.rows);
    }
    Ok(())
}

#[test]
fn ejs_classic_globals_are_shared_and_module_locals_shadow_them() -> TestResult {
    let p = Project::new()?;
    let source = "<script>function shared(){}</script>\n<script>function classic(){shared();}</script>\n<script type=\"module\">function importedGlobal(){shared();}</script>\n<script type=\"module\">function shared(){} function local(){shared();}</script>";
    p.write("Main.ejs", source)?;
    p.build(false)?;
    for (caller, expected_line) in [("classic", 1), ("importedGlobal", 1), ("local", 4)] {
        let rows = p.query(Query::Deps {
            selector: selector("Main.ejs", caller),
        })?;
        let edges = relationships(&rows);
        let edge = edges
            .iter()
            .find(|e| {
                e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == "shared")
            })
            .ok_or("missing shared call")?;
        assert_eq!(
            edge.target.as_ref().ok_or("target")?.definition.line,
            expected_line,
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn ejs_script_boundaries_do_not_merge_line_comments_or_incomplete_syntax() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.ejs",
        "<script>function first(){} // end</script><script>function second(){first();}</script>",
    )?;
    let before = p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.ejs", "second"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == "first")),
        "{:?}",
        rows.rows
    );
    p.write(
        "Main.ejs",
        "<script>function broken(){</script><script>}</script>",
    )?;
    assert!(p.build(false).is_err());
    let rows = p.query(Query::Symbols {
        file: "Main.ejs".into(),
    })?;
    assert_eq!(
        rows.metadata.ok_or("metadata")?.generation,
        Some(before.generation)
    );
    Ok(())
}

#[test]
fn ejs_script_slices_preserve_original_unicode_crlf_and_inline_columns() -> TestResult {
    let p = Project::new()?;
    let source = "<h1>한글</h1>\r\n<script>function target(){}</script><script type=\"module\">function boot(){target();}</script>";
    p.write("Main.ejs", source)?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.ejs", "boot"),
    })?;
    let edges = relationships(&rows);
    let edge = edges.iter().find(|e| e.relation == "calls").ok_or("call")?;
    let start = source.find("target();").ok_or("source position")?;
    assert_eq!(edge.site.span.start_byte, start);
    assert_eq!(edge.site.span.line, 2);
    assert_eq!(
        edge.site.span.column,
        start - source.find('\n').ok_or("line")?
    );
    assert_eq!(
        &source[edge.site.span.start_byte..edge.site.span.end_byte],
        "target"
    );
    Ok(())
}

#[test]
fn ejs_rendered_private_module_does_not_taint_an_independent_static_module() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs", "<script type=\"module\">const value=<%= value %>; function a(){} function rendered(){a();}</script>\n<script type=\"module\">function b(){} function stable(){b();}</script>")?;
    p.build(false)?;
    for (caller, expected) in [("rendered", "candidate"), ("stable", "resolved")] {
        let rows = p.query(Query::Explain {
            selector: selector("Main.ejs", caller),
        })?;
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "calls" && e.resolution == expected),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn ejs_server_strings_cannot_close_a_client_script_and_src_bodies_are_not_executed() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs", "<script>const text=<%- '</script>' %>; function boot(){}</script><script src=\"remote.js\">function ignored(){}</script>")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "Main.ejs".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="boot")),
        "{:?}",
        rows.rows
    );
    assert!(
        !rows
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="ignored")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn ejs_rendering_diagnostics_and_scope_changes_follow_incremental_generations() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs", "<script>const value=<%= value %>; function shared(){}</script><script type=\"module\">function boot(){shared();}</script>")?;
    p.build(false)?;
    let diagnostic = |p: &Project| -> std::result::Result<bool, Box<dyn std::error::Error>> {
        let mut rows = Collected::default();
        p.engine
            .status(&p.options(), true, &Cancellation::default(), &mut rows)?;
        Ok(rows
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Diagnostic(d) if d.code=="ejs_rendered_client")))
    };
    assert!(diagnostic(&p)?);
    p.write("Main.ejs", "<script type=\"module\">function shared(){}</script><script type=\"module\">function boot(){shared();}</script>")?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    assert!(!diagnostic(&p)?);
    let query = || Query::Explain {
        selector: selector("Main.ejs", "boot"),
    };
    let before = p.query(query())?;
    let normalized = |rows: &Collected| {
        relationships(rows)
            .iter()
            .map(|e| {
                (
                    e.relation.clone(),
                    e.resolution.clone(),
                    e.target.as_ref().map(|t| t.logical_key.clone()),
                    e.site.span.start_byte,
                )
            })
            .collect::<Vec<_>>()
    };
    assert!(
        !relationships(&before)
            .iter()
            .any(|e| e.relation == "calls" && e.target.is_some())
    );
    p.build(true)?;
    assert_eq!(normalized(&before), normalized(&p.query(query())?));
    Ok(())
}
