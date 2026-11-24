// Purpose: 조건부 주석·상속 설정 부재·복합 타입 오버로드의 입력 경계 검증.
// Pattern: Integration Test.
// Usage: engine 통합 검사에서 유효 입력 보존과 설정 복구를 확인.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/application/analysis/configurations.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;

#[test]
fn multiline_directive_comments_preserve_both_conditional_regions() -> TestResult {
    let p = Project::new()?;
    p.write("main.c", "#if FEATURE /* a comment\ncontinued here */\nint enabled(void) { return 1; }\n#else\nint disabled(void) { return 0; }\n#endif\n")?;
    let report = p.build(false)?;
    assert_eq!(report.coverage.failed_files, 0);
    let rows = p.query(Query::Symbols {
        file: "main.c".into(),
    })?;
    for name in ["enabled", "disabled"] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == name))
        );
    }
    p.write(
        "main.c",
        "#if FEATURE /* never closed\nint invalid(void) {}\n",
    )?;
    p.write("valid.py", "def retained(): pass\n")?;
    assert_eq!(p.build(false)?.coverage.partial_files, 1);
    Ok(())
}

#[test]
fn missing_inherited_configuration_is_partial_without_guessing_package_fallback() -> TestResult {
    let p = Project::new()?;
    p.write("tsconfig.json", r#"{"extends":"./generated/base.json"}"#)?;
    p.write(
        "package.json",
        r#"{"name":"target","exports":"./wrong.ts"}"#,
    )?;
    p.write("wrong.ts", "export function target() {}")?;
    p.write("right.ts", "export function target() {}")?;
    p.write(
        "caller.ts",
        "import { target } from 'target'; export function caller() { target(); }",
    )?;
    let initial = p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("caller.ts", "caller"),
    })?;
    assert!(!rows.summary.as_ref().ok_or("summary")?.complete);
    assert!(relationships(&rows).iter().all(|r| r.target.is_none()));
    p.write(
        "generated/base.json",
        r#"{"compilerOptions":{"paths":{"target":["../right.ts"]}}}"#,
    )?;
    let changed = p.build(false)?;
    assert!(changed.generation > initial.generation);
    let rows = p.query(Query::Deps {
        selector: selector("caller.ts", "caller"),
    })?;
    assert!(rows.summary.as_ref().ok_or("summary")?.complete);
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|s| s.file == "right.ts"))
    );
    assert_eq!(
        normalized_index(&changed)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn newline_generic_overloads_after_compound_return_types_remain_distinct() -> TestResult {
    let p = Project::new()?;
    p.write("types.ts", "interface Respond { <T>(value: T): Response & Typed<T>\n<U>(value: U): Response | Typed<U>\n<V>(value: V): Typed<V> }\n")?;
    assert_eq!(p.build(false)?.coverage.failed_files, 0);
    Ok(())
}

#[test]
fn literal_contents_and_soft_keywords_are_not_misread_as_structure() -> TestResult {
    let mut failures = Vec::new();
    for (file, source, name) in [
        (
            "empty.zig",
            "const Value = struct {}; const Selected = if (flag) struct {} else other;",
            "Value",
        ),
        (
            "region.cs",
            "class Value {}\n#region metadata\n#endregion metadata",
            "Value",
        ),
        (
            "nul.php",
            "<?php function value() { return 'a\0b'; }",
            "value",
        ),
        ("binary.rb", "# encoding: binary\nclass Value; end", "Value"),
        (
            "context.kt",
            "fun value() { context[Handler]?.let { it.run(); return }; suspend { fail() }() }",
            "value",
        ),
        (
            "guard.kt",
            "fun value(e: Throwable) = when(e) { is Error if ready() -> 1; else -> 0 }",
            "value",
        ),
        (
            "set.kt",
            "fun value() {\n val set = { 1 }\n val update = { 2 }\n set() shouldBe update()\n}",
            "value",
        ),
        (
            "text.swift",
            "func value() -> String { return \"\\(start)-\\(end)/*\" }\n// */ is ordinary comment content\nfunc after() {}",
            "after",
        ),
        (
            "dollar.swift",
            "struct Value { let _$id = 1; @_lifetime(borrow scope) init() {} }",
            "Value",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        let report = p.build(false)?;
        let rows = p.query(Query::Symbols { file: file.into() });
        if report.coverage.failed_files != 0
            || report.coverage.partial_files != 0
            || !rows.as_ref().is_ok_and(|r| {
                r.rows
                    .iter()
                    .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == name))
            })
        {
            let names = rows.as_ref().map(|r| {
                r.rows
                    .iter()
                    .filter_map(|r| match r {
                        QueryRow::Symbol(s) => Some(s.name.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            });
            failures.push(format!(
                "{file}: failed={}, partial={}, names={names:?}",
                report.coverage.failed_files, report.coverage.partial_files
            ));
        }
    }
    assert!(failures.is_empty(), "valid forms rejected: {failures:?}");
    Ok(())
}

#[test]
fn macro_provided_erlang_headers_remain_visible_as_partial_context() -> TestResult {
    let p = Project::new()?;
    p.write("sample.erl", "-module(sample).\nrun() -> try work() catch ?WITH_STACKTRACE(Class, Reason, Stack) handle(Reason) end.\nstart(Name) -> ?CASE_START(Name) {error, Reason} -> handle(Reason) end.\nwork() -> ok.\nhandle(_) -> ok.\n")?;
    let report = p.build(false)?;
    assert_eq!(report.coverage.failed_files, 0);
    assert_eq!(report.coverage.fragment_files, 1);
    let rows = p.query(Query::Symbols {
        file: "sample.erl".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "run/0"))
    );
    assert!(!rows.summary.ok_or("summary")?.complete);
    Ok(())
}
