// Purpose: 정적 .NET Compile 소속과 partial·XAML·증분 경계 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/application/analysis/dotnet_projects.rs.

use crate::support::*;
use kodstar_core::*;

fn key(p: &Project, file: &str) -> std::result::Result<String, Box<dyn std::error::Error>> {
    let rows = p.query(Query::Explain {
        selector: selector(file, "App::Worker"),
    })?;
    rows.rows
        .iter()
        .find_map(|row| match row {
            QueryRow::Symbol(symbol) if symbol.name == "Worker" => Some(symbol.logical_key.clone()),
            _ => None,
        })
        .ok_or_else(|| "missing Worker".into())
}

fn sources(p: &Project) -> TestResult {
    p.write(
        "app/First.cs",
        "namespace App; partial class Worker { void Caller() { this.Target(); } }",
    )?;
    p.write(
        "app/Second.cs",
        "namespace App; partial class Worker { void Target() {} }",
    )?;
    Ok(())
}

#[test]
fn compile_remove_changes_membership_without_reparsing_unchanged_sources() -> TestResult {
    let p = Project::new()?;
    sources(&p)?;
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"/>")?;
    p.build(false)?;
    assert_eq!(key(&p, "app/First.cs")?, key(&p, "app/Second.cs")?);
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup><Compile Remove=\"Second.cs\"/></ItemGroup></Project>")?;
    let update = p.build(false)?;
    assert_eq!(update.parsed_files, 1);
    let first = key(&p, "app/First.cs")?;
    let excluded = key(&p, "app/Second.cs")?;
    assert_ne!(first, excluded);
    let rows = p.query(Query::Deps {
        selector: selector("app/First.cs", "App::Worker::Caller"),
    })?;
    assert!(
        !relationships(&rows).iter().any(|r| r.relation == "calls"
            && r.target.as_ref().is_some_and(|t| t.file == "app/Second.cs"))
    );
    p.build(true)?;
    assert_eq!(first, key(&p, "app/First.cs")?);
    assert_eq!(excluded, key(&p, "app/Second.cs")?);
    assert_eq!(p.build(false)?.parsed_files, 0);
    Ok(())
}

#[test]
fn compile_include_exclude_and_remove_follow_item_order() -> TestResult {
    let p = Project::new()?;
    sources(&p)?;
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup><ItemGroup><Compile Include=\"*.cs\" Exclude=\"Second.cs\"/></ItemGroup></Project>")?;
    p.build(false)?;
    assert_ne!(key(&p, "app/First.cs")?, key(&p, "app/Second.cs")?);
    p.write("app/App.csproj", "<Project><ItemGroup><Compile Include=\"*.cs\"/><Compile Remove=\"Second.cs\"/><Compile Include=\"Second.cs\"/></ItemGroup></Project>")?;
    p.build(false)?;
    assert_eq!(key(&p, "app/First.cs")?, key(&p, "app/Second.cs")?);
    Ok(())
}

#[test]
fn a_linked_source_can_belong_to_one_explicit_project() -> TestResult {
    let p = Project::new()?;
    sources(&p)?;
    p.write(
        "shared/Linked.cs",
        "namespace App; partial class Worker { void Linked() {} }",
    )?;
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup><Compile Include=\"..\\shared\\Linked.cs\"/></ItemGroup></Project>")?;
    p.build(false)?;
    assert_eq!(key(&p, "app/First.cs")?, key(&p, "shared/Linked.cs")?);
    p.write(
        "other/Other.csproj",
        "<Project><ItemGroup><Compile Include=\"../shared/Linked.cs\"/></ItemGroup></Project>",
    )?;
    p.build(false)?;
    assert_ne!(key(&p, "app/First.cs")?, key(&p, "shared/Linked.cs")?);
    Ok(())
}

#[test]
fn conditional_or_dynamic_compile_membership_never_merges_partial_types() -> TestResult {
    let p = Project::new()?;
    sources(&p)?;
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup Condition=\"'$(Configuration)' == 'Debug'\"><Compile Remove=\"Second.cs\"/></ItemGroup></Project>")?;
    p.build(false)?;
    assert_ne!(key(&p, "app/First.cs")?, key(&p, "app/Second.cs")?);
    let mut rows = Collected::default();
    p.engine
        .status(&p.options(), true, &Cancellation::default(), &mut rows)?;
    assert!(
        rows.rows.iter().any(
            |r| matches!(r, QueryRow::Diagnostic(d) if d.code == "dotnet_membership_uncertain")
        )
    );
    assert_eq!(
        rows.summary.as_ref().ok_or("summary")?.result_count as usize,
        rows.rows.len()
    );
    Ok(())
}

#[test]
fn xaml_events_share_the_unique_project_context_with_code_behind() -> TestResult {
    let p = Project::new()?;
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"/>")?;
    p.write(
        "app/Main.xaml.cs",
        "namespace App; partial class Main { void OnClick() {} }",
    )?;
    p.write("app/Main.xaml", "<Window xmlns:x=\"http://schemas.microsoft.com/winfx/2006/xaml\" x:Class=\"App.Main\"><Button Click=\"OnClick\"/></Window>")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("app/Main.xaml", "app/Main.xaml"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.relation == "event_handler"
                && r.resolution == "resolved"
                && r.target
                    .as_ref()
                    .is_some_and(|t| t.file == "app/Main.xaml.cs")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn default_items_and_literal_brackets_do_not_turn_into_glob_character_classes() -> TestResult {
    let p = Project::new()?;
    sources(&p)?;
    p.write("app/part[1].cs", "namespace App; partial class Worker {}")?;
    p.write("app/part1.cs", "namespace App; partial class Worker {}")?;
    p.write("app/App.csproj", "<Project><Sdk Name=\"Microsoft.NET.Sdk\"/><PropertyGroup><EnableDefaultItems>false</EnableDefaultItems></PropertyGroup><ItemGroup><Compile Include=\"First.cs;part[1].cs\"/></ItemGroup></Project>")?;
    p.build(false)?;
    assert_eq!(key(&p, "app/First.cs")?, key(&p, "app/part[1].cs")?);
    assert_ne!(key(&p, "app/First.cs")?, key(&p, "app/part1.cs")?);
    assert_ne!(key(&p, "app/First.cs")?, key(&p, "app/Second.cs")?);
    Ok(())
}

#[test]
fn adding_and_deleting_directory_customization_invalidates_membership() -> TestResult {
    let p = Project::new()?;
    sources(&p)?;
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"/>")?;
    p.build(false)?;
    assert_eq!(key(&p, "app/First.cs")?, key(&p, "app/Second.cs")?);
    p.write("Directory.Build.props", "<Project><PropertyGroup><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup></Project>")?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    assert_ne!(key(&p, "app/First.cs")?, key(&p, "app/Second.cs")?);
    std::fs::remove_file(p.root.join("Directory.Build.props"))?;
    assert_eq!(p.build(false)?.parsed_files, 0);
    assert_eq!(key(&p, "app/First.cs")?, key(&p, "app/Second.cs")?);
    Ok(())
}

#[test]
fn over_budget_patterns_report_uncertainty_without_partial_merging() -> TestResult {
    let p = Project::new()?;
    sources(&p)?;
    let patterns = "unused.cs;".repeat(1025);
    p.write("app/App.csproj", &format!("<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup><Compile Include=\"{patterns}\"/></ItemGroup></Project>"))?;
    p.build(false)?;
    assert_ne!(key(&p, "app/First.cs")?, key(&p, "app/Second.cs")?);
    let mut rows = Collected::default();
    p.engine
        .status(&p.options(), true, &Cancellation::default(), &mut rows)?;
    assert!(
        rows.rows.iter().any(
            |r| matches!(r, QueryRow::Diagnostic(d) if d.code == "dotnet_membership_uncertain")
        )
    );
    Ok(())
}
