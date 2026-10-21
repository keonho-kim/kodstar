// Purpose: namespace 소유 범위·inline module import의 오연결 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn global_csharp_types_are_visible_across_files_without_foreign_namespace_leaks() -> TestResult {
    let project = Project::new()?;
    project.write("other/Worker.cs", "class Worker { public void Run() {} }")?;
    project.write(
        "foreign/Worker.cs",
        "namespace Foreign { class Worker { public void Run() {} } }",
    )?;
    project.write(
        "Main.cs",
        "class Main { void caller(Worker value) { value.Run(); } }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("Main.cs", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", rows.rows);
    assert_eq!(
        calls[0].target.as_ref().ok_or("target")?.file,
        "other/Worker.cs"
    );
    Ok(())
}

#[test]
fn rust_relative_paths_follow_the_declaration_scope_and_exact_keywords() -> TestResult {
    let project = Project::new()?;
    project.write("src/lib.rs","mod superb { pub fn target() {} } mod cratefoo { pub fn target() {} } use superb::target as first; use cratefoo::target as second; mod outer { mod sibling { pub fn target() {} } mod inner { mod local { pub fn target() {} } use super::sibling::target as invoke; use self::local::target as local_call; pub fn caller() { invoke(); local_call(); } } } fn top() { first(); second(); }")?;
    project.build(false)?;
    for (caller, targets) in [
        ("top", vec!["superb::target", "cratefoo::target"]),
        (
            "outer::inner::caller",
            vec!["outer::sibling::target", "outer::inner::local::target"],
        ),
    ] {
        let rows = project.query(Query::Deps {
            selector: selector("src/lib.rs", caller),
        })?;
        let calls: Vec<_> = relationships(&rows)
            .into_iter()
            .filter(|e| e.relation == "calls")
            .collect();
        assert_eq!(calls.len(), 2, "{:?}", rows.rows);
        for target in targets {
            assert!(calls.iter().any(|e| {
                e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == target)
            }));
        }
    }
    Ok(())
}

#[test]
fn rust_nested_external_mod_and_namespaced_reexports_keep_their_owner() -> TestResult {
    let project = Project::new()?;
    project.write("src/lib.rs","mod outer { mod leaf; pub fn caller() { leaf::target(); } } mod values { pub fn first() {} pub fn second() {} } mod api { pub use crate::values::first as target; } mod wrong { pub use crate::values::second as target; } use crate::api::target as invoke; fn top() { invoke(); }")?;
    project.write("src/outer/leaf.rs", "pub fn target() {}")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("src/lib.rs", "top"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", rows.rows);
    assert_eq!(
        calls[0].target.as_ref().ok_or("target")?.qualified_name,
        "values::first"
    );
    let rows = project.query(Query::Deps {
        selector: selector("src/lib.rs", "outer::caller"),
    })?;
    assert!(relationships(&rows).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.file == "src/outer/leaf.rs")
    }));
    Ok(())
}

#[test]
fn same_namespace_visibility_does_not_guess_across_foreign_namespaces() -> TestResult {
    let project = Project::new()?;
    project.write(
        "A.cs",
        "namespace A { class Service { public void Run() {} } }",
    )?;
    project.write(
        "B.cs",
        "namespace B { class Service { public void Run() {} } }",
    )?;
    project.write(
        "Main.cs",
        "namespace A; class Main { void caller(Service value) { value.Run(); } }",
    )?;
    project.write(
        "Other.cs",
        "namespace App { class Main { void caller(Service value) { value.Run(); } } }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("Main.cs", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", rows.rows);
    assert_eq!(
        calls[0].target.as_ref().ok_or("target")?.qualified_name,
        "A::Service::Run"
    );
    assert!(
        relationships(&project.query(Query::Deps {
            selector: selector("Other.cs", "caller")
        })?)
        .is_empty()
    );
    Ok(())
}

#[test]
fn php_semicolon_namespaces_keep_each_following_definition_in_its_scope() -> TestResult {
    let project = Project::new()?;
    project.write("types.php","<?php namespace A; class Service { public function run() {} } namespace B; class Service { public function run() {} }")?;
    project.write("main.php","<?php namespace App; use A\\Service as Owner; function caller(Owner $value) { $value->run(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.php", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", rows.rows);
    assert_eq!(
        calls[0].target.as_ref().ok_or("target")?.qualified_name,
        "A::Service::run"
    );
    Ok(())
}

#[test]
fn csharp_using_selects_only_members_of_the_requested_block_namespace() -> TestResult {
    let project = Project::new()?;
    project.write("Types.cs","namespace A { public class Service { public void Run() {} } } namespace B { public class Service { public void Run() {} } }")?;
    project.write(
        "Main.cs",
        "using A; namespace App { class Main { void caller(Service value) { value.Run(); } } }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("Main.cs", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", rows.rows);
    assert_eq!(
        calls[0].target.as_ref().ok_or("target")?.qualified_name,
        "A::Service::Run"
    );
    project.write("Types.cs","namespace A { public class Other {} } namespace B { public class Service { public void Run() {} } }")?;
    project.build(false)?;
    assert!(
        relationships(&project.query(Query::Deps {
            selector: selector("Main.cs", "caller")
        })?)
        .is_empty()
    );
    Ok(())
}

#[test]
fn php_braced_namespaces_keep_imported_types_separate() -> TestResult {
    let project = Project::new()?;
    project.write("types.php","<?php namespace A { class Service { public function run() {} } } namespace B { class Service { public function run() {} } }")?;
    project.write("main.php","<?php namespace App { use A\\Service as Owner; function caller(Owner $value) { $value->run(); } }")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.php", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", rows.rows);
    assert_eq!(
        calls[0].target.as_ref().ok_or("target")?.qualified_name,
        "A::Service::run"
    );
    Ok(())
}

#[test]
fn rust_inline_modules_and_nested_pub_use_resolve_their_own_scope() -> TestResult {
    let project = Project::new()?;
    project.write("src/lib.rs","mod api { pub fn target() {} } mod wrong { pub fn target() {} } use crate::api::target as invoke; fn top() { invoke(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("src/lib.rs", "top"),
    })?;
    assert_eq!(relationships(&rows).len(), 1, "{:?}", rows.rows);
    assert_eq!(
        relationships(&rows)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .qualified_name,
        "api::target"
    );
    Ok(())
}
