// Purpose: Razor 정적 지시문·handler·component·프로젝트 소속과 원본 위치 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/formats/components/razor/mod.rs.

use crate::support::*;
use kodstar_core::*;

fn project(p: &Project) -> TestResult {
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"/>")?;
    Ok(())
}

#[test]
fn razor_model_injection_inheritance_and_route_keep_explicit_targets() -> TestResult {
    let p = Project::new()?;
    project(&p)?;
    p.write("app/Types.cs", "namespace Shared { class Person { public void Run() {} } class Service { public void Work() {} } class Base {} }")?;
    p.write("app/Main.cshtml", "@using Shared\n@inject Service Store\n@model Person\n@inherits Base\n@page \"/people\"\n@functions { void Caller() { Model.Run(); Store.Work(); } }\n")?;
    p.build(false)?;
    let root = p.query(Query::Explain {
        selector: selector("app/Main.cshtml", "app/Main.cshtml"),
    })?;
    for (target, relation) in [
        ("Person", "references"),
        ("Service", "references"),
        ("Base", "inherits"),
    ] {
        assert!(
            relationships(&root)
                .iter()
                .any(|e| e.relation == relation
                    && e.target.as_ref().is_some_and(|t| t.name == target)),
            "{:?}",
            root.rows
        );
    }
    assert!(
        relationships(&root)
            .iter()
            .any(|e| e.relation == "references"
                && e.external.as_deref() == Some("route:razor:/people")),
        "{:?}",
        root.rows
    );
    let rows = p.query(Query::Deps {
        selector: selector("app/Main.cshtml", "Caller"),
    })?;
    for name in ["Run", "Work"] {
        assert!(relationships(&rows).iter().any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == name)), "{:?}", rows.rows);
    }
    Ok(())
}

#[test]
fn razor_using_alias_and_static_imports_are_visible_inside_functions() -> TestResult {
    let p = Project::new()?;
    project(&p)?;
    p.write("app/Types.cs", "namespace Shared { class Person { public void Run() {} } class Factories { public static void Make() {} } }")?;
    p.write("app/Main.cshtml", "@using Alias = Shared.Person\n@using static Shared.Factories\n@model Alias\n@functions { void Caller(){ Make(); Model.Run(); } }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("app/Main.cshtml", "Caller"),
    })?;
    for name in ["Make", "Run"] {
        assert!(relationships(&rows).iter().any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == name)), "{:?}", rows.rows);
    }
    Ok(())
}

#[test]
fn razor_static_event_handlers_resolve_to_embedded_methods_with_original_ranges() -> TestResult {
    let p = Project::new()?;
    project(&p)?;
    let source =
        "<p>한글</p>\n<button @onclick=\"@Clicked\">ok</button>\n@code { void Clicked() {} }\n";
    p.write("app/Main.razor", source)?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("app/Main.razor", "app/Main.razor"),
    })?;
    let edges = relationships(&rows);
    let edge = edges
        .iter()
        .find(|e| {
            e.relation == "event_handler" && e.target.as_ref().is_some_and(|t| t.name == "Clicked")
        })
        .ok_or_else(|| format!("{:?}", rows.rows))?;
    assert_eq!(
        &source[edge.site.span.start_byte..edge.site.span.end_byte],
        "Clicked"
    );
    assert_eq!(edge.site.span.line, 2);
    Ok(())
}

#[test]
fn razor_comments_and_script_strings_do_not_create_fake_directives_or_methods() -> TestResult {
    let p = Project::new()?;
    p.write("Main.razor", "@*\n@model Missing\n@code { not C sharp }\n<Ghost/>\n*@\n<!--\n@inject Missing Ghost\n-->\n<script>const text='@code { not C sharp }';</script>\n@code { void Real() {} }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.razor", "Main.razor"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.expression.contains("Missing") || e.expression.contains("Ghost")),
        "{:?}",
        rows.rows
    );
    let rows = p.query(Query::Symbols {
        file: "Main.razor".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "Real"))
    );
    Ok(())
}

#[test]
fn explicit_razor_namespaces_link_component_files_in_the_same_project() -> TestResult {
    let p = Project::new()?;
    project(&p)?;
    p.write(
        "app/Types.cs",
        "namespace Shared { class NamespaceMarker {} }",
    )?;
    p.write("app/Widget.razor", "@namespace Shared\n<div>widget</div>\n")?;
    p.write("app/Main.razor", "@using Shared\n<Widget/>\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("app/Main.razor", "app/Main.razor"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.file == "app/Widget.razor")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn same_directory_razor_components_are_candidates_without_proven_namespace() -> TestResult {
    let p = Project::new()?;
    project(&p)?;
    p.write("app/Widget.razor", "<div/>")?;
    p.write("app/nested/Widget.razor", "<div/>")?;
    p.write("app/Main.razor", "<Widget/>")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("app/Main.razor", "app/Main.razor"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.resolution == "candidate"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.file == "app/Widget.razor")),
        "{:?}",
        rows.rows
    );
    assert!(!relationships(&rows).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.file == "app/nested/Widget.razor")
    }));
    Ok(())
}

#[test]
fn new_razor_model_targets_reresolve_without_reparsing_the_view() -> TestResult {
    let p = Project::new()?;
    project(&p)?;
    p.write(
        "app/Main.cshtml",
        "@model Shared.Person\n@functions { void Caller(){ Model.Run(); } }\n",
    )?;
    p.build(false)?;
    p.write(
        "app/Types.cs",
        "namespace Shared { class Person { public void Run() {} } }",
    )?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    let query = || Query::Explain {
        selector: selector("app/Main.cshtml", "Caller"),
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
            .any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == "Run")),
        "{:?}",
        rows.rows
    );
    p.build(true)?;
    assert_eq!(normalized(&rows), normalized(&p.query(query())?));
    Ok(())
}
