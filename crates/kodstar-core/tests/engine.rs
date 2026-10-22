// Purpose: 엔진 조회·증분·rollback·경로 계약 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-core --test engine.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/tests/cases/languages.rs, crates/kodstar-core/tests/cases/components.rs.

#[path = "cases/annotations.rs"]
mod annotations;
#[path = "cases/astro.rs"]
mod astro;
#[path = "cases/blade.rs"]
mod blade;
#[path = "cases/cache.rs"]
mod cache;
#[path = "cases/callable_values.rs"]
mod callable_values;
#[path = "cases/component_links.rs"]
mod component_links;
#[path = "cases/components.rs"]
mod components;
#[path = "cases/configuration_paths.rs"]
mod configuration_paths;
#[path = "cases/consistency.rs"]
mod consistency;
#[path = "cases/coverage.rs"]
mod coverage;
#[path = "cases/cxx.rs"]
mod cxx;
#[path = "cases/cxx_guards.rs"]
mod cxx_guards;
#[path = "cases/cxx_identities.rs"]
mod cxx_identities;
#[path = "cases/declarations.rs"]
mod declarations;
#[path = "cases/dispatch.rs"]
mod dispatch;
#[path = "cases/dotnet_projects.rs"]
mod dotnet_projects;
#[path = "cases/ejs.rs"]
mod ejs;
#[path = "cases/ejs_boundaries.rs"]
mod ejs_boundaries;
#[path = "cases/ejs_scopes.rs"]
mod ejs_scopes;
#[path = "cases/elixir.rs"]
mod elixir;
#[path = "cases/erlang.rs"]
mod erlang;
#[path = "cases/erlang_types.rs"]
mod erlang_types;
#[path = "cases/external_targets.rs"]
mod external_targets;
#[path = "cases/fields.rs"]
mod fields;
#[path = "cases/formats.rs"]
mod formats;
#[path = "cases/getattr_values.rs"]
mod getattr_values;
#[path = "cases/go_incremental.rs"]
mod go_incremental;
#[path = "cases/go_interfaces.rs"]
mod go_interfaces;
#[path = "cases/go_paths.rs"]
mod go_paths;
#[path = "cases/history_impact.rs"]
mod history_impact;
#[path = "cases/history_operations.rs"]
mod history_operations;
#[path = "cases/history_paths.rs"]
mod history_paths;
#[path = "cases/history_recovery.rs"]
mod history_recovery;
#[path = "cases/history_streaming.rs"]
mod history_streaming;
#[path = "cases/history_upgrade.rs"]
mod history_upgrade;
#[path = "cases/history_watch.rs"]
mod history_watch;
#[path = "cases/identities.rs"]
mod identities;
#[path = "cases/incremental_inputs.rs"]
mod incremental_inputs;
#[path = "cases/javascript.rs"]
mod javascript;
#[path = "cases/kotlin.rs"]
mod kotlin;
#[path = "cases/kotlin_names.rs"]
mod kotlin_names;
#[path = "cases/languages.rs"]
mod languages;
#[path = "cases/lisp.rs"]
mod lisp;
#[path = "cases/lisp_bindings.rs"]
mod lisp_bindings;
#[path = "cases/lisp_packages.rs"]
mod lisp_packages;
#[path = "cases/local_history.rs"]
mod local_history;
#[path = "cases/module_rules.rs"]
mod module_rules;
#[path = "cases/namespaces.rs"]
mod namespaces;
#[path = "cases/optimization.rs"]
mod optimization;
#[path = "cases/ownership.rs"]
mod ownership;
#[path = "cases/package_paths.rs"]
mod package_paths;
#[path = "cases/pascal.rs"]
mod pascal;
#[path = "cases/php.rs"]
mod php;
#[path = "cases/php_traits.rs"]
mod php_traits;
#[path = "cases/privacy.rs"]
mod privacy;
#[path = "cases/python_comprehensions.rs"]
mod python_comprehensions;
#[path = "cases/python_exports.rs"]
mod python_exports;
#[path = "cases/python_fixtures.rs"]
mod python_fixtures;
#[path = "cases/python_members.rs"]
mod python_members;
#[path = "cases/python_scope.rs"]
mod python_scope;
#[path = "cases/razor.rs"]
mod razor;
#[path = "cases/razor_ranges.rs"]
mod razor_ranges;
#[path = "cases/refresh_reuse.rs"]
mod refresh_reuse;
#[path = "cases/requests.rs"]
mod requests;
#[path = "cases/resolution.rs"]
mod resolution;
#[path = "cases/resources.rs"]
mod resources;
#[path = "cases/return_values.rs"]
mod return_values;
#[path = "cases/ruby.rs"]
mod ruby;
#[path = "cases/rust_macros.rs"]
mod rust_macros;
#[path = "cases/rust_namespaces.rs"]
mod rust_namespaces;
#[path = "cases/scala.rs"]
mod scala;
mod support;
#[path = "cases/swift.rs"]
mod swift;
#[path = "cases/swift_packages.rs"]
mod swift_packages;
#[path = "cases/synchronization.rs"]
mod synchronization;
#[path = "cases/type_rules.rs"]
mod type_rules;
#[path = "cases/watch.rs"]
mod watch;
#[path = "cases/zig.rs"]
mod zig;
use kodstar_core::*;
use std::path::{Path, PathBuf};
use support::*;

#[test]
fn rust_direction_recursion_and_use_ranges_are_preserved() -> TestResult {
    let project = Project::new()?;
    let source = "pub fn target() {}\npub fn caller() { target(); caller(); }\n";
    project.write("src/main.rs", source)?;
    let report = project.build(false)?;
    assert_eq!(report.parsed_files, 1);
    let deps = project.query(Query::Deps {
        selector: selector("src/main.rs", "caller"),
    })?;
    let edges = relationships(&deps);
    assert_eq!(edges.len(), 2);
    for edge in edges {
        assert_eq!(edge.source.name, "caller");
        assert_eq!(edge.relation, "calls");
        assert_eq!(edge.resolution, "resolved");
        let name = &edge.target.as_ref().ok_or("missing target")?.name;
        assert_eq!(
            &source.as_bytes()[edge.site.span.start_byte..edge.site.span.end_byte],
            name.as_bytes()
        );
    }
    let refs = project.query(Query::Refs {
        selector: selector("src/main.rs", "target"),
    })?;
    assert_eq!(relationships(&refs).len(), 1);
    assert_eq!(relationships(&refs)[0].direction, "in");
    Ok(())
}

#[test]
fn parameter_shadowing_does_not_bind_to_a_same_named_global() -> TestResult {
    let project = Project::new()?;
    project.write(
        "api.py",
        "def target():\n    pass\n\ndef caller(target):\n    target()\n",
    )?;
    project.build(false)?;
    let deps = project.query(Query::Deps {
        selector: selector("api.py", "caller"),
    })?;
    assert!(relationships(&deps).is_empty());
    let explained = project.query(Query::Explain {
        selector: selector("api.py", "caller"),
    })?;
    assert!(relationships(&explained).iter().any(|edge| {
        edge.resolution == "unresolved"
            && edge
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("shadows"))
    }));
    Ok(())
}

#[test]
fn changed_import_targets_are_reresolved_without_reparsing_the_caller() -> TestResult {
    let project = Project::new()?;
    project.write(
        "api.ts",
        "import { target as invoke } from './dep';\nexport function caller() { invoke(); }\n",
    )?;
    project.write("dep.ts", "export function target() {}\n")?;
    project.build(false)?;
    let initial = project.query(Query::Deps {
        selector: selector("api.ts", "caller"),
    })?;
    assert_eq!(relationships(&initial).len(), 1);
    project.write("dep.ts", "export function other() {}\n")?;
    let update = project.build(false)?;
    assert_eq!(update.parsed_files, 1);
    assert_eq!(update.coverage.reused_files, 1);
    let missing = project.query(Query::Deps {
        selector: selector("api.ts", "caller"),
    })?;
    assert!(relationships(&missing).is_empty());
    project.write("dep.ts", "export function target() {}\n")?;
    project.build(false)?;
    let restored = project.query(Query::Deps {
        selector: selector("api.ts", "caller"),
    })?;
    assert_eq!(relationships(&restored).len(), 1);
    Ok(())
}

#[test]
fn parse_failure_preserves_previous_generation_and_stale_access_is_explicit() -> TestResult {
    let project = Project::new()?;
    project.write("src/main.rs", "pub fn stable() {}\n")?;
    let before = project.build(false)?;
    project.write("src/main.rs", "pub fn broken( {\n")?;
    assert!(project.build(false).is_err());
    let cached = project.query(Query::Symbols {
        file: PathBuf::from("src/main.rs"),
    })?;
    let metadata = cached.metadata.ok_or("missing metadata")?;
    assert_eq!(metadata.generation, Some(before.generation));
    assert_eq!(metadata.freshness, "not_checked");
    assert!(
        cached
            .rows
            .iter()
            .any(|row| matches!(row,QueryRow::Symbol(symbol) if symbol.name=="stable"))
    );
    let mut sink = Collected::default();
    let error = project
        .engine
        .query(
            &project.options(),
            &Query::Symbols {
                file: PathBuf::from("src/main.rs"),
            },
            &QueryOptions::default(),
            &Cancellation::default(),
            &mut sink,
        )
        .err()
        .ok_or("refresh should fail")?;
    assert_eq!(error.kind, ErrorKind::Operation);
    assert!(sink.rows.is_empty());
    Ok(())
}

#[test]
fn no_index_query_does_not_initialize_a_cache() -> TestResult {
    let project = Project::new()?;
    project.write("api.py", "def sample(): pass\n")?;
    let result = project.query(Query::Symbols {
        file: PathBuf::from("api.py"),
    });
    assert_eq!(
        result.err().ok_or("missing failure")?.kind,
        ErrorKind::NotFound
    );
    assert!(!project.directory.join("cache").exists());
    Ok(())
}

#[test]
fn unchanged_build_does_not_advance_generation() -> TestResult {
    let project = Project::new()?;
    project.write("api.py", "def sample(): pass\n")?;
    let initial = project.build(false)?;
    let reused = project.build(false)?;
    assert!(!reused.changed);
    assert_eq!(reused.generation, initial.generation);
    assert_eq!(reused.parsed_files, 0);
    Ok(())
}

#[test]
fn byte_path_identity_and_symlink_root_are_not_display_string_identity() -> TestResult {
    let project = Project::new()?;
    project.write("api.py", "def sample(): pass\n")?;
    project.build(false)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link = project.directory.join("linked-project");
        symlink(&project.root, &link)?;
        let options = ProjectOptions::new(link);
        let report = project.engine.init(
            &options,
            InitOptions::default(),
            &Cancellation::default(),
            &mut |_| Ok(()),
        )?;
        assert!(!report.changed);
    }
    assert!(Path::new(&project.root).is_absolute());
    Ok(())
}
