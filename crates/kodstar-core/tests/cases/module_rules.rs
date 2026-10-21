// Purpose: 언어별 명시적 파일·모듈 import와 호출 대상 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn c_includes_bind_header_declarations_and_never_guess_foreign_extensions() -> TestResult {
    let project = Project::new()?;
    project.write("api.h", "void target(void);")?;
    project.write(
        "main.c",
        "#include \"api.h\"\nvoid caller(void) { target(); }\n",
    )?;
    project.write("wrong.ts", "export function target() {}")?;
    project.write(
        "other.c",
        "#include \"wrong\"\nvoid other(void) { target(); }\n",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.c", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.file == "api.h"))
    );
    assert!(
        relationships(&project.query(Query::Deps {
            selector: selector("other.c", "other")
        })?)
        .is_empty()
    );
    Ok(())
}

#[test]
fn qualified_imports_and_explicit_receiver_types_keep_the_target_namespace() -> TestResult {
    for (file, source, dep, definition, caller) in [
        (
            "Main.java",
            "package app; import api.Service; class Main { void caller(Service value) { value.run(); } }",
            "api/Service.java",
            "package api; public class Service { public void run() {} }",
            "Main::caller",
        ),
        (
            "Main.cs",
            "namespace App; using Api; class Main { void caller(Service value) { value.Run(); } }",
            "Api.cs",
            "namespace Api; class Service { public void Run() {} }",
            "App::Main::caller",
        ),
        (
            "main.scala",
            "package app\nimport api.{Service as Owner}\nclass Main { def caller(value: Owner): Unit = { value.run() } }\n",
            "api.scala",
            "package api\nclass Service { def run(): Unit = {} }\n",
            "Main::caller",
        ),
        (
            "main.php",
            "<?php namespace App; use Api\\Service as Owner; function caller(Owner $value) { $value->run(); }",
            "api.php",
            "<?php namespace Api; class Service { public function run() {} }",
            "caller",
        ),
        (
            "main.kt",
            "package app\nimport api.Service as Owner\nfun caller(value: Owner) { value.run() }\n",
            "api.kt",
            "package api\nclass Service {\n fun run() {}\n}\n",
            "caller",
        ),
    ] {
        let project = Project::new()?;
        project.write(file, source)?;
        project.write(dep, definition)?;
        project.build(false)?;
        let rows = project.query(Query::Deps {
            selector: selector(file, caller),
        })?;
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.file == dep)),
            "{file}: {:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn go_default_import_uses_the_declared_package_name() -> TestResult {
    let project = Project::new()?;
    project.write("go.mod", "module example.test/project\n")?;
    project.write("renamed/api.go", "package service\nfunc Target() {}\n")?;
    project.write("main.go","package main\nimport \"example.test/project/renamed\"\nfunc caller() { service.Target(); renamed.Target() }\n")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.go", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e
                .target
                .as_ref()
                .is_some_and(|t| t.file == "renamed/api.go"))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn go_module_import_alias_resolves_only_the_declared_package_path() -> TestResult {
    let project = Project::new()?;
    project.write("go.mod", "module example.test/project\ngo 1.25\n")?;
    project.write("service/api.go", "package service\nfunc Target() {}\n")?;
    project.write("other/api.go", "package other\nfunc Target() {}\n")?;
    project.write("main.go", "package main\nimport api \"example.test/project/service\"\nfunc caller() { api.Target() }\n")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.go", "caller"),
    })?;
    assert_eq!(relationships(&rows).len(), 1);
    assert_eq!(
        relationships(&rows)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .file,
        "service/api.go"
    );
    Ok(())
}

#[test]
fn ruby_relative_require_and_zig_static_import_resolve_real_targets() -> TestResult {
    for (file, source, dep, definition) in [
        (
            "main.rb",
            "require_relative 'api'\ndef caller\n Api.target\nend\n",
            "api.rb",
            "class Api\n def self.target\n end\nend\n",
        ),
        (
            "main.zig",
            "const api = @import(\"api.zig\"); pub fn caller() void { api.target(); }",
            "api.zig",
            "pub fn target() void {}",
        ),
    ] {
        let project = Project::new()?;
        project.write(file, source)?;
        project.write(dep, definition)?;
        project.build(false)?;
        let rows = project.query(Query::Deps {
            selector: selector(file, "caller"),
        })?;
        assert_eq!(relationships(&rows).len(), 1, "{file}: {:?}", rows.rows);
        assert_eq!(
            relationships(&rows)[0]
                .target
                .as_ref()
                .ok_or("target")?
                .file,
            dep
        );
    }
    Ok(())
}

#[test]
fn rust_external_mod_pub_use_and_macro_namespaces_keep_distinct_bindings() -> TestResult {
    let project = Project::new()?;
    project.write("src/lib.rs", "mod api; pub use api::target; macro_rules! target { () => {} } fn caller() { api::target(); target(); target!(); }")?;
    project.write("src/api.rs", "pub fn target() {}")?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("src/lib.rs", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert!(
        calls.iter().all(|e| e.resolution == "resolved"),
        "{:?}",
        rows.rows
    );
    assert_eq!(
        calls
            .iter()
            .filter(|e| e.target.as_ref().is_some_and(|t| t.kind == "function"))
            .map(|e| e.count)
            .sum::<u64>(),
        2
    );
    assert_eq!(
        calls
            .iter()
            .filter(|e| e.target.as_ref().is_some_and(|t| t.kind == "macro"))
            .map(|e| e.count)
            .sum::<u64>(),
        1
    );
    Ok(())
}
