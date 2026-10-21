// Purpose: C# partial 객체의 위치·프로젝트 경계·삭제와 증분 정체성 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

fn symbols(rows: &Collected) -> Vec<&Symbol> {
    rows.rows
        .iter()
        .filter_map(|r| {
            if let QueryRow::Symbol(s) = r {
                Some(s)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn partial_types_share_members_and_preserve_every_definition() -> TestResult {
    let p = Project::new()?;
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\" />")?;
    p.write(
        "app/First.cs",
        "namespace App; partial class Worker { void Caller() { this.Target(); } }",
    )?;
    p.write(
        "app/nested/Second.cs",
        "namespace App; partial class Worker { void Target() {} }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("app/First.cs", "App::Worker::Caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "App::Worker::Target")),
        "{:?}",
        rows.rows
    );
    let first = p.query(Query::Explain {
        selector: selector("app/First.cs", "App::Worker"),
    })?;
    let second = p.query(Query::Explain {
        selector: selector("app/nested/Second.cs", "App::Worker"),
    })?;
    let first = symbols(&first).into_iter().next().ok_or("first")?;
    let second = symbols(&second).into_iter().next().ok_or("second")?;
    assert_eq!(first.id, second.id);
    assert_eq!(first.logical_key, second.logical_key);
    let value = serde_json::to_value(first)?;
    assert_eq!(
        value["definitions"].as_array().ok_or("definitions")?.len(),
        2
    );
    Ok(())
}

#[test]
fn partial_representative_deletion_retains_identity_and_remaining_members() -> TestResult {
    let p = Project::new()?;
    p.write(
        "A.cs",
        "namespace App; partial class Worker { void Old() {} }",
    )?;
    p.write(
        "B.cs",
        "namespace App; partial class Worker { void Target() {} }",
    )?;
    p.build(false)?;
    let query = || Query::Explain {
        selector: selector("B.cs", "App::Worker"),
    };
    let before = p.query(query())?;
    let key = symbols(&before)[0].logical_key.clone();
    std::fs::remove_file(p.root.join("A.cs"))?;
    p.build(false)?;
    let incremental = p.query(query())?;
    assert_eq!(key, symbols(&incremental)[0].logical_key);
    p.build(true)?;
    assert_eq!(key, symbols(&p.query(query())?)[0].logical_key);
    Ok(())
}

#[test]
fn same_named_partial_types_in_distinct_projects_are_not_merged() -> TestResult {
    let p = Project::new()?;
    for dir in ["one", "two"] {
        p.write(&format!("{dir}/App.csproj"), "<Project />")?;
        p.write(
            &format!("{dir}/Worker.cs"),
            "namespace App; partial class Worker {}",
        )?;
    }
    p.build(false)?;
    let one = p.query(Query::Explain {
        selector: selector("one/Worker.cs", "App::Worker"),
    })?;
    let two = p.query(Query::Explain {
        selector: selector("two/Worker.cs", "App::Worker"),
    })?;
    assert_ne!(symbols(&one)[0].logical_key, symbols(&two)[0].logical_key);
    Ok(())
}

#[test]
fn representative_search_order_does_not_depend_on_incremental_database_ids() -> TestResult {
    let p = Project::new()?;
    let source = "namespace App; partial class Worker {}";
    p.write("A.cs", source)?;
    p.write("B.cs", source)?;
    p.build(false)?;
    p.write("A.cs", &format!("{source}\n"))?;
    p.build(false)?;
    let query = || Query::Search {
        name: "Worker".into(),
        prefix: false,
    };
    let incremental = p.query(query())?;
    assert_eq!(symbols(&incremental).len(), 1);
    assert_eq!(symbols(&incremental)[0].file, "A.cs");
    p.build(true)?;
    assert_eq!(symbols(&p.query(query())?)[0].file, "A.cs");
    Ok(())
}

#[test]
fn project_membership_does_not_force_unchanged_csharp_files_to_reparse() -> TestResult {
    let p = Project::new()?;
    p.write("app/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\" />")?;
    p.write("app/Main.cs", "class Main { void Run() {} }")?;
    p.build(false)?;
    let unchanged = p.build(false)?;
    assert_eq!(unchanged.parsed_files, 0);
    assert!(!unchanged.changed);
    Ok(())
}
