// Purpose: 명시적 binding·scope·모듈의 대상 해석 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn go_files_share_only_their_declared_package_namespace() -> TestResult {
    let project = Project::new()?;
    project.write(
        "api.go",
        "package api\nfunc target() {}\nfunc caller() { target() }",
    )?;
    project.write(
        "api_test.go",
        "package api_test\nfunc external() { target() }",
    )?;
    project.write("more.go", "package api\nfunc shared() { target() }")?;
    project.build(false)?;
    assert!(
        relationships(&project.query(Query::Deps {
            selector: selector("api_test.go", "external")
        })?)
        .is_empty()
    );
    let shared = project.query(Query::Deps {
        selector: selector("more.go", "shared"),
    })?;
    assert_eq!(relationships(&shared).len(), 1);
    assert_eq!(
        relationships(&shared)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .file,
        "api.go"
    );
    Ok(())
}

#[test]
fn typescript_module_names_do_not_make_unimported_definitions_visible() -> TestResult {
    let project = Project::new()?;
    project.write("same.ts", "export function caller() { target(); }")?;
    project.write("same.tsx", "export function target() {}")?;
    project.build(false)?;
    assert!(
        relationships(&project.query(Query::Deps {
            selector: selector("same.ts", "caller")
        })?)
        .is_empty()
    );
    Ok(())
}

#[test]
fn rust_impl_members_and_self_resolve_to_their_explicit_owner() -> TestResult {
    let project = Project::new()?;
    project.write("lib.rs", "struct Service; impl Service { fn target() {} fn caller(&self) { Self::target(); self.target(); } }")?;
    project.build(false)?;
    let rows = project.query(Query::Symbols {
        file: "lib.rs".into(),
    })?;
    let owner = rows
        .rows
        .iter()
        .find_map(|row| match row {
            QueryRow::Symbol(symbol) if symbol.name == "Service" => Some(symbol.id),
            _ => None,
        })
        .ok_or("owner")?;
    assert!(rows.rows.iter().any(|row| matches!(row, QueryRow::Symbol(symbol) if symbol.qualified_name == "Service::caller" && symbol.owner == Some(owner))));
    let calls = project.query(Query::Deps {
        selector: selector("lib.rs", "Service::caller"),
    })?;
    assert_eq!(
        relationships(&calls)
            .iter()
            .filter(|edge| edge.relation == "calls"
                && edge
                    .target
                    .as_ref()
                    .is_some_and(|target| target.name == "target"))
            .map(|edge| edge.count)
            .sum::<u64>(),
        2
    );
    let mut members = Collected::default();
    project.engine.query(
        &project.options(),
        &Query::Related {
            selector: selector("lib.rs", "Service"),
        },
        &QueryOptions {
            refresh: Refresh::None,
            members: true,
            direction: Some(Direction::Out),
            relations: vec![RelationKind::Calls],
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut members,
    )?;
    assert_eq!(
        relationships(&members)
            .iter()
            .map(|edge| edge.count)
            .sum::<u64>(),
        2
    );
    Ok(())
}

#[test]
fn declaration_names_are_not_uses_but_recursive_field_types_are() -> TestResult {
    let project = Project::new()?;
    let source = "struct Node { next: Option<Box<Node>> }";
    project.write("lib.rs", source)?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("lib.rs", "Node"),
    })?;
    let uses: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|edge| {
            edge.relation == "uses_type"
                && edge.direction == "out"
                && edge
                    .target
                    .as_ref()
                    .is_some_and(|target| target.name == "Node")
        })
        .collect();
    assert_eq!(uses.len(), 1);
    assert_eq!(
        uses[0].site.span.start_byte,
        source.rfind("Node").ok_or("recursive use")?
    );
    Ok(())
}

#[test]
fn generic_parameters_shadow_same_named_module_types() -> TestResult {
    let project = Project::new()?;
    project.write("lib.rs", "struct T; fn caller<T>(input: T) {}")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("lib.rs", "caller"),
    })?;
    assert!(!relationships(&rows).iter().any(|edge| {
        edge.target
            .as_ref()
            .is_some_and(|target| target.name == "T")
    }));
    Ok(())
}

#[test]
fn sibling_reexport_chains_preserve_all_candidates() -> TestResult {
    let project = Project::new()?;
    project.write(
        "caller.ts",
        "import { target } from './barrel'; function caller() { target(); }",
    )?;
    project.write(
        "barrel.ts",
        "export { target } from './one'; export { target } from './two';",
    )?;
    project.write("one.ts", "export { target } from './leaf-one';")?;
    project.write("two.ts", "export { target } from './leaf-two';")?;
    project.write("leaf-one.ts", "export function target() {}")?;
    project.write("leaf-two.ts", "export function target() {}")?;
    project.build(false)?;
    let query = || Query::Explain {
        selector: selector("caller.ts", "caller"),
    };
    let rows = project.query(query())?;
    let edges = relationships(&rows);
    assert_eq!(edges.len(), 2);
    assert!(edges.iter().all(|edge| edge.resolution == "ambiguous"));
    let mut files: Vec<_> = edges
        .iter()
        .filter_map(|edge| edge.target.as_ref().map(|symbol| symbol.file.as_str()))
        .collect();
    files.sort_unstable();
    assert_eq!(files, ["leaf-one.ts", "leaf-two.ts"]);
    project.write("leaf-two.ts", "export function other() {}")?;
    project.build(false)?;
    let rows = project.query(query())?;
    let edges = relationships(&rows);
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].target.as_ref().ok_or("target")?.file,
        "leaf-one.ts"
    );
    Ok(())
}

#[test]
fn an_unbound_receiver_does_not_resolve_to_a_same_named_file() -> TestResult {
    let project = Project::new()?;
    project.write("api.ts", "export function target() {}")?;
    project.write("caller.ts", "function caller() { api.target(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("caller.ts", "caller"),
    })?;
    let edges = relationships(&rows);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].resolution, "unresolved");
    assert!(edges[0].target.is_none());
    assert!(edges[0].external.is_none());
    Ok(())
}

#[test]
fn nested_python_classes_do_not_capture_an_outer_class_namespace() -> TestResult {
    let project = Project::new()?;
    project.write(
        "main.py",
        "def target(): pass\nclass Outer:\n    def target(): pass\n    class Inner:\n        def caller(self):\n            target()\n",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.py", "Outer::Inner::caller"),
    })?;
    let edges = relationships(&rows);
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].target.as_ref().ok_or("target")?.qualified_name,
        "target"
    );
    Ok(())
}

#[test]
fn rust_module_functions_keep_function_kind_and_lexical_bindings() -> TestResult {
    let project = Project::new()?;
    project.write(
        "lib.rs",
        "mod api { fn target() {} fn caller() { target(); } }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Symbols {
        file: "lib.rs".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|row| matches!(row, QueryRow::Symbol(symbol)
        if symbol.qualified_name == "api::target" && symbol.kind == "function"))
    );
    let rows = project.query(Query::Deps {
        selector: selector("lib.rs", "api::caller"),
    })?;
    assert_eq!(relationships(&rows).len(), 1);
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
