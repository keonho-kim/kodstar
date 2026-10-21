// Purpose: EJS client helper·동적 import·비코드 script와 embedded TSX 경계 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/formats/components/ejs/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn client_include_is_not_the_ejs_server_helper() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs", "<script>include('partial');</script>")?;
    p.write("partial.ejs", "<p/>")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.ejs", "Main.ejs"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "template_include"
                || e.external.as_deref() == Some("builtin:ejs:include")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn rendered_module_strings_do_not_create_an_import_of_the_placeholder_value() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs","<script type=\"module\">import {run} from '<%= moduleName %>'; function boot(){ run(); }</script>")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.ejs", "Main.ejs"),
    })?;
    assert!(
        !relationships(&rows).iter().any(|e| e
            .external
            .as_deref()
            .is_some_and(|k| k.contains("module:0"))),
        "{:?}",
        rows.rows
    );
    let rows = p.query(Query::Explain {
        selector: selector("Main.ejs", "boot"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.resolution == "resolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn script_data_does_not_choose_the_parser_for_executable_client_code() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs","<script type=\"application/json\" lang=\"ts\">{\"text\":\"data\"}</script><script>function boot(){return <div/>}</script>")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "Main.ejs".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s)if s.name=="boot")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn embedded_tsx_uses_the_selected_official_tsx_grammar() -> TestResult {
    let p = Project::new()?;
    p.write("Panel.tsx", "export function Panel(){return <div/>}")?;
    p.write("Main.vue","<script lang=\"tsx\">import {Panel} from './Panel'; function render(){return <Panel/>}</script><template><div/></template>")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.vue", "render"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.name == "Panel" && t.file == "Panel.tsx")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn ejs_output_expressions_accept_the_optional_trailing_semicolon() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.ejs",
        "<% function target(){return 1;} %><%= target(); %>",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.ejs", "Main.ejs"),
    })?;
    assert!(relationships(&rows).iter().any(|e|e.relation=="calls"&&e.target.as_ref().is_some_and(|t|t.name=="target")),"{:?}",rows.rows);
    Ok(())
}

#[test]
fn real_script_tags_ignore_case_and_script_shapes_in_server_strings_are_not_client_code()
-> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs","<% const text='<script>function fake(){}</script>'; %>\n<SCRIPT>function target(){} function boot(){target();}</SCRIPT>")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "Main.ejs".into(),
    })?;
    assert!(
        !rows
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s)if s.name=="fake"))
    );
    let rows = p.query(Query::Deps {
        selector: selector("Main.ejs", "boot"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "resolved"
            && e.target.as_ref().is_some_and(|t| t.name == "target")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn ejs_require_and_dynamic_imports_link_only_explicit_module_paths() -> TestResult {
    let p = Project::new()?;
    p.write("lib/api.js", "export function run(){}")?;
    p.write(
        "views/Main.ejs",
        "<% const api=require('../lib/api'); const lazy=import('../lib/api.js'); %>",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("views/Main.ejs", "views/Main.ejs"),
    })?;
    let edges = relationships(&rows);
    let imports: Vec<_> = edges
        .iter()
        .filter(|e| {
            e.relation == "imports" && e.target.as_ref().is_some_and(|t| t.file == "lib/api.js")
        })
        .collect();
    assert_eq!(
        imports.iter().map(|e| e.count).sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn runtime_generated_client_identifiers_do_not_publish_fabricated_names() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs", "<script>function stable(){}</script>")?;
    let before = p.build(false)?;
    p.write(
        "Main.ejs",
        "<script>function generated<%= suffix %>(){}</script>",
    )?;
    assert!(p.build(false).is_err());
    let rows = p.query(Query::Symbols {
        file: "Main.ejs".into(),
    })?;
    assert_eq!(
        rows.metadata.as_ref().ok_or("metadata")?.generation,
        Some(before.generation)
    );
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s)if s.name=="stable"))
    );
    Ok(())
}

#[test]
fn a_native_language_override_does_not_enable_template_helpers_by_filename() -> TestResult {
    let p = Project::new()?;
    p.write("Main.ejs", "include('partial');")?;
    p.write("partial.ejs", "<p/>")?;
    let config = p.directory.join("settings.toml");
    std::fs::write(
        &config,
        "[language_overrides]\n\"Main.ejs\"=\"javascript\"\n",
    )?;
    let mut options = p.options();
    options.config = Some(config);
    p.engine.build(
        &options,
        BuildOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    let mut rows = Collected::default();
    p.engine.query(
        &options,
        &Query::Explain {
            selector: selector("Main.ejs", "Main.ejs"),
        },
        &QueryOptions {
            refresh: Refresh::None,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut rows,
    )?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "template_include"
                || e.external.as_deref() == Some("builtin:ejs:include")),
        "{:?}",
        rows.rows
    );
    Ok(())
}
