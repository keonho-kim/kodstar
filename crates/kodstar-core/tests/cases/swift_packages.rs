// Purpose: 정적 SwiftPM target 경계·import·설정 무효화 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

fn manifest(dependencies: &str) -> String {
    format!(
        "import PackageDescription\nlet package=Package(name: \"Fixture\",targets: [.target(name: \"Core\"),.target(name: \"App\",dependencies: [{dependencies}])])\n"
    )
}

fn run_calls(p: &Project, file: &str) -> TestResult {
    let rows = p.query(Query::Deps {
        selector: selector(file, "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "resolved"
            && e.target
                .as_ref()
                .is_some_and(|t| t.name == "run" && t.file.ends_with("Worker.swift"))),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn swift_target_members_share_a_module_across_nested_directories() -> TestResult {
    let p = Project::new()?;
    p.write("Package.swift", &manifest("\"Core\""))?;
    p.write(
        "Sources/Core/Models/Worker.swift",
        "public class Worker {\n public func run() {}\n}\n",
    )?;
    p.write(
        "Sources/Core/Calls/Main.swift",
        "func caller(value: Worker){ value.run() }\n",
    )?;
    p.write(
        "Sources/App/Main.swift",
        "import Core\nfunc caller(value: Worker){ value.run() }\n",
    )?;
    p.build(false)?;
    run_calls(&p, "Sources/Core/Calls/Main.swift")?;
    run_calls(&p, "Sources/App/Main.swift")?;
    Ok(())
}

#[test]
fn swift_unlisted_target_dependencies_do_not_link_sibling_modules() -> TestResult {
    let p = Project::new()?;
    p.write("Package.swift", &manifest(""))?;
    p.write(
        "Sources/Core/Worker.swift",
        "public class Worker {\n public func run() {}\n}\n",
    )?;
    p.write(
        "Sources/App/Main.swift",
        "import Core\nfunc caller(value: Worker){ value.run() }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Sources/App/Main.swift", "caller"),
    })?;
    assert!(
        !relationships(&rows).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.file.starts_with("Sources/Core"))),
        "{:?}",
        rows.rows
    );
    p.write("Package.swift", &manifest("\"Core\""))?;
    p.build(false)?;
    run_calls(&p, "Sources/App/Main.swift")?;
    let incremental = normalized_rows(&p.query(Query::Deps {
        selector: selector("Sources/App/Main.swift", "caller"),
    })?);
    p.build(true)?;
    assert_eq!(
        incremental,
        normalized_rows(&p.query(Query::Deps {
            selector: selector("Sources/App/Main.swift", "caller")
        })?)
    );
    Ok(())
}

#[test]
fn swift_custom_sources_and_excludes_control_membership() -> TestResult {
    let p = Project::new()?;
    p.write("Package.swift","import PackageDescription\nlet package=Package(name: \"Fixture\",targets: [.target(name: \"Core\",path: \"Code\",exclude: [\"Ignored\"],sources: [\"Models\",\"Calls\"])])\n")?;
    p.write(
        "Code/Models/Worker.swift",
        "class Worker {\n func run() {}\n}\n",
    )?;
    p.write(
        "Code/Calls/Main.swift",
        "func caller(value: Worker){ value.run() }\n",
    )?;
    p.write(
        "Code/Ignored/Worker.swift",
        "class Worker {\n func run() {}\n}\n",
    )?;
    p.write(
        "Code/Other/Worker.swift",
        "class Worker {\n func run() {}\n}\n",
    )?;
    p.build(false)?;
    run_calls(&p, "Code/Calls/Main.swift")?;
    Ok(())
}

#[test]
fn swift_dynamic_sources_and_overlapping_targets_do_not_guess_membership() -> TestResult {
    for targets in [
        ".target(name: \"Core\",sources: selectedSources)",
        ".target(name: \"Core\",path: \"Sources/Core\"),.target(name: \"Other\",path: \"Sources/Core\")",
    ] {
        let p = Project::new()?;
        p.write("Package.swift",&format!("import PackageDescription\nlet package=Package(name: \"Fixture\",targets: [{targets}])\n"))?;
        p.write(
            "Sources/Core/Worker.swift",
            "class Worker {\n func run() {}\n}\n",
        )?;
        p.write(
            "Sources/Core/Main.swift",
            "func caller(value: Worker){ value.run() }\n",
        )?;
        p.build(false)?;
        let rows = p.query(Query::Deps {
            selector: selector("Sources/Core/Main.swift", "caller"),
        })?;
        assert!(
            !relationships(&rows).iter().any(|e| e
                .target
                .as_ref()
                .is_some_and(|t| t.file.ends_with("Worker.swift"))),
            "{:?}",
            rows.rows
        );
        let unchanged = p.build(false)?;
        assert_eq!(unchanged.parsed_files, 0);
        assert!(!unchanged.changed);
    }
    Ok(())
}

#[test]
fn swift_internal_declarations_and_members_do_not_escape_their_target() -> TestResult {
    let p = Project::new()?;
    p.write("Package.swift", &manifest("\"Core\""))?;
    p.write("Sources/Core/Worker.swift","public class Worker {\n func hidden() {}\n public func run() {}\n}\nclass Hidden {\n public func run() {}\n}\n")?;
    p.write("Sources/App/Main.swift","import Core\nfunc caller(value: Worker, hidden: Hidden){ value.run(); value.hidden(); hidden.run() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Sources/App/Main.swift", "caller"),
    })?;
    let targets = relationships(&rows)
        .iter()
        .filter(|e| e.relation == "calls")
        .filter_map(|e| e.target.as_ref())
        .map(|s| s.qualified_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(targets, vec!["Worker::run"], "{:?}", rows.rows);
    Ok(())
}

#[test]
fn swift_nested_manifest_boundaries_and_deleted_manifests_are_recomputed() -> TestResult {
    let p = Project::new()?;
    p.write("Package.swift", &manifest("\"Core\""))?;
    p.write(
        "Sources/Core/Worker.swift",
        "public class Worker {\n public func run() {}\n}\n",
    )?;
    p.write(
        "Sources/App/Main.swift",
        "import Core\nfunc caller(value: Worker){ value.run() }\n",
    )?;
    p.write("Sources/App/Package.swift","import PackageDescription\nlet package=Package(name: \"Nested\",targets: [.target(name: \"App\",path: \".\")])\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Sources/App/Main.swift", "caller"),
    })?;
    assert!(!relationships(&rows).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|s| s.file.starts_with("Sources/Core"))
    }));
    std::fs::remove_file(p.root.join("Sources/App/Package.swift"))?;
    p.build(false)?;
    run_calls(&p, "Sources/App/Main.swift")?;
    let incremental = normalized_rows(&p.query(Query::Deps {
        selector: selector("Sources/App/Main.swift", "caller"),
    })?);
    p.build(true)?;
    assert_eq!(
        incremental,
        normalized_rows(&p.query(Query::Deps {
            selector: selector("Sources/App/Main.swift", "caller")
        })?)
    );
    Ok(())
}

fn normalized_rows(rows: &Collected) -> Vec<(String, String, String, String)> {
    let mut normalized = relationships(rows)
        .iter()
        .map(|e| {
            (
                e.expression.clone(),
                e.relation.clone(),
                e.resolution.clone(),
                e.target
                    .as_ref()
                    .map(|t| format!("{}:{}:{}", t.file, t.qualified_name, t.logical_key))
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    normalized.sort();
    normalized
}

#[test]
fn swift_selected_imports_and_module_qualification_use_the_declared_target() -> TestResult {
    for (import, expression) in [
        ("import class Core.Worker", "Worker.run()"),
        ("import Core", "Core.Worker.run()"),
    ] {
        let p = Project::new()?;
        p.write("Package.swift", &manifest("\"Core\""))?;
        p.write(
            "Sources/Core/Worker.swift",
            "public class Worker {\n public static func run() {}\n}\n",
        )?;
        p.write(
            "Sources/App/Main.swift",
            &format!("{import}\nfunc caller(){{ {expression} }}\n"),
        )?;
        p.build(false)?;
        run_calls(&p, "Sources/App/Main.swift")?;
    }
    Ok(())
}

#[test]
fn swift_conditional_target_dependencies_remain_candidates() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Package.swift",
        &manifest(".target(name: \"Core\",condition: .when(platforms: [.macOS]))"),
    )?;
    p.write(
        "Sources/Core/Worker.swift",
        "public class Worker {\n public func run() {}\n}\n",
    )?;
    p.write(
        "Sources/App/Main.swift",
        "import Core\nfunc caller(value: Worker){ value.run() }\n",
    )?;
    p.build(false)?;
    let mut rows = Collected::default();
    p.engine.query(
        &p.options(),
        &Query::Deps {
            selector: selector("Sources/App/Main.swift", "caller"),
        },
        &QueryOptions {
            refresh: Refresh::None,
            include_candidates: true,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut rows,
    )?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "candidate"
            && e.target.as_ref().is_some_and(|s| s.name == "run")),
        "{:?}",
        rows.rows
    );
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "calls" && e.resolution == "resolved")
    );
    Ok(())
}
