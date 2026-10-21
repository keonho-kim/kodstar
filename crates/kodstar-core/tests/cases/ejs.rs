// Purpose: EJS include·출력·server/client scope·원본 위치와 증분 경계 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/formats/components/ejs/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn ejs_static_includes_are_relative_to_the_template_and_keep_original_ranges() -> TestResult {
    let p = Project::new()?;
    let source = "<h1>한글</h1>\n<%- include('partials/header') %>\n<% include('./partials/footer.ejs'); %>\n";
    p.write("views/Main.ejs", source)?;
    p.write("views/partials/header.ejs", "<header/>")?;
    p.write("views/partials/footer.ejs", "<footer/>")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("views/Main.ejs", "views/Main.ejs"),
    })?;
    for target in ["views/partials/header.ejs", "views/partials/footer.ejs"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "template_include"
                    && e.resolution == "resolved"
                    && e.target.as_ref().is_some_and(|t| t.file == target)),
            "{:?}",
            rows.rows
        );
    }
    let edges = relationships(&rows);
    let edge = edges
        .iter()
        .find(|e| e.relation == "template_include" && e.site.span.line == 2)
        .ok_or("include")?;
    assert_eq!(
        &source[edge.site.span.start_byte..edge.site.span.end_byte],
        "partials/header"
    );
    Ok(())
}

#[test]
fn ejs_scriptlets_and_adjacent_output_expressions_keep_server_call_scopes() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs","<% function greet(value){ return value; } function render(){ %>\n<p><%= greet('one') %><%= greet('two') %></p>\n<% } %>\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.ejs", "render"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(
                |e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == "greet")
            )
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn ejs_comments_and_escaped_tags_do_not_become_code_or_includes() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.ejs",
        "<%# include('fake') %>\n<p><%% not a tag %></p>\n<% function real() { return 1; } %>\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "Main.ejs".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s)if s.name=="real"))
    );
    let rows = p.query(Query::Explain {
        selector: selector("Main.ejs", "Main.ejs"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "template_include")
    );
    Ok(())
}

#[test]
fn ejs_include_shadowing_remains_an_ordinary_function_call() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.ejs",
        "<% function include(path){ return path; } %><%- include('partial') %>",
    )?;
    p.write("partial.ejs", "<p/>")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.ejs", "Main.ejs"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "template_include"),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows).iter().any(
            |e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == "include")
        )
    );
    Ok(())
}

#[test]
fn ejs_server_and_client_functions_do_not_capture_each_other() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs","<% function serverOnly(){} function serverCaller(){ clientOnly(); } %>\n<script>function clientOnly(){} function clientCaller(){ serverOnly(); }</script>\n")?;
    p.build(false)?;
    for caller in ["serverCaller", "clientCaller"] {
        let rows = p.query(Query::Explain {
            selector: selector("Main.ejs", caller),
        })?;
        assert!(
            !relationships(&rows)
                .iter()
                .any(|e| e.relation == "calls" && e.target.is_some()),
            "{caller}: {:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn ejs_rendered_client_output_is_syntactic_placeholder_and_relations_are_candidates() -> TestResult
{
    let p = Project::new()?;
    p.write("Main.ejs","<script>\n const data=<%- value %>;\n function target(){}\n function boot(){ target(); }\n</script>\n<script type=\"application/json\">{\"not\": \"JavaScript block\"}</script>\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.ejs", "boot"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "candidate"
            && e.target.as_ref().is_some_and(|t| t.name == "target")),
        "{:?}",
        rows.rows
    );
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.resolution == "resolved")
    );
    Ok(())
}

#[test]
fn new_ejs_include_targets_reresolve_without_reparsing_the_template() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs", "<%- include('partial') %>")?;
    p.build(false)?;
    p.write("partial.ejs", "<p/>")?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    let query = || Query::Explain {
        selector: selector("Main.ejs", "Main.ejs"),
    };
    let rows = p.query(query())?;
    let normalized = |rows: &Collected| {
        relationships(rows)
            .iter()
            .map(|e| {
                (
                    e.relation.clone(),
                    e.resolution.clone(),
                    e.target.as_ref().map(|t| t.logical_key.clone()),
                )
            })
            .collect::<Vec<_>>()
    };
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "template_include" && e.resolution == "resolved"),
        "{:?}",
        rows.rows
    );
    p.build(true)?;
    assert_eq!(normalized(&rows), normalized(&p.query(query())?));
    Ok(())
}
