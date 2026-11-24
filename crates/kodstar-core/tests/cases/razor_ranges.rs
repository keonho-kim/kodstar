// Purpose: Razor C# 범위의 문자열·주석과 파일 binding 수명·rollback 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/formats/components/razor/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn razor_code_ranges_ignore_comment_braces_and_verbatim_or_raw_string_contents() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.razor",
        r####"@code {
void Caller() {
 var path = @"C:\temp\";
 var raw = """ } @model Missing """;
 /* } */ // }
 Target();
}
void Target() {}
}
"####,
    )?;
    let build = p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.razor", "Caller"),
    })?;
    assert!(relationships(&rows).iter().any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == "Target")), "{:?}", rows.rows);
    p.write("Main.razor", "@code { void Broken() {")?;
    assert_eq!(p.build(false)?.coverage.failed_files, 1);
    let rows = p.query(Query::Symbols {
        file: "Main.razor".into(),
    })?;
    assert_eq!(
        rows.metadata.as_ref().ok_or("metadata")?.generation,
        Some(build.generation + 1)
    );
    assert!(
        rows.rows
            .iter()
            .all(|r| !matches!(r, QueryRow::Symbol(s) if s.name == "Target"))
    );
    Ok(())
}

#[test]
fn razor_implicit_model_and_injected_fields_are_visible_before_their_directives() -> TestResult {
    let p = Project::new()?;
    p.write("Types.cs", "namespace App { class ModelType { public void Run() {} } class Service { public void Work() {} } }")?;
    p.write("Main.cshtml", "@functions { void Caller() { Model.Run(); Store.Work(); } }\n@model App.ModelType\n@inject App.Service Store\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.cshtml", "Caller"),
    })?;
    for name in ["Run", "Work"] {
        assert!(relationships(&rows).iter().any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t|t.name == name)), "{:?}", rows.rows);
    }
    Ok(())
}

#[test]
fn razor_directive_types_do_not_cross_distinct_project_contexts() -> TestResult {
    let p = Project::new()?;
    p.write("one/One.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"/>")?;
    p.write("two/Two.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"/>")?;
    p.write(
        "one/Main.cshtml",
        "@model Shared.Person\n@functions { void Caller(){ Model.Run(); } }\n",
    )?;
    p.write(
        "two/Types.cs",
        "namespace Shared { class Person { public void Run() {} } }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("one/Main.cshtml", "Caller"),
    })?;
    assert!(
        !relationships(&rows).iter().any(|e| e.target.is_some()),
        "{:?}",
        rows.rows
    );
    Ok(())
}
