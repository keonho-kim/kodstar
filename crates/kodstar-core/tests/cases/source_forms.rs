// Purpose: 실프로젝트의 문법·전처리 fragment와 혼합 입력 형식 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 검사에서 이름 있는 정의와 입력 완전성을 함께 확인.
// Related: crates/kodstar-core/tests/support/mod.rs, scripts/grammars/generate.py.

use crate::support::*;
use kodstar_core::*;

#[test]
fn valid_current_source_forms_keep_their_definitions() -> TestResult {
    let mut failures = Vec::new();
    for (file, source, expected) in [
        (
            "main.jsx",
            "function List() { return <li class={active ? 'yes' : 'no'} for='item'/>; }",
            "List",
        ),
        (
            "main.tsx",
            "function Main() { return <><UI.Button/><Panel class='selected'/></>; }",
            "Main",
        ),
        (
            "variance.ts",
            "interface Schema<in Input, out Output> { parse(value: Input): Output; }",
            "Schema",
        ),
        (
            "nested.ts",
            "function test() { expectTypeOf(fn).toBe<<Code extends number>(statusCode: Code) => Code>(); const n = left << right; }",
            "test",
        ),
        (
            "cast.ts",
            "function compare(left: number, right: unknown) { return left < <number>right; }",
            "compare",
        ),
        (
            "available.c",
            "int target(void) { return __builtin_available(macOS 10.12, iOS 5, *) ? 1 : 0; }",
            "target",
        ),
        (
            "overloads.ts",
            "interface Get<E> {\n <Key extends keyof E['Variables']>(key: Key): E['Variables'][Key]\n <Key extends keyof Map>(key: Key): Map[Key]\n}",
            "Get",
        ),
        (
            "Main.java",
            "class Main { void target(Class<?> @Nullable... types) {} }",
            "target",
        ),
        (
            "Main.kt",
            "class Item\nfun make(value: Item, text: String=\"value\"): Item = value\n",
            "make",
        ),
        (
            "context.kt",
            "class Project\ncontext(project: Project)\nfun target() {}\n",
            "target",
        ),
        (
            "constructor.kt",
            "@JvmInline\nvalue class Carrier<out T>\n@PublishedApi internal constructor(@PublishedApi internal val holder: Any?) {}",
            "Carrier",
        ),
        (
            "main.php",
            "<?php function target($data) { return new Binary($data, Binary::TYPE_GENERIC); }",
            "target",
        ),
        (
            "main.rb",
            "class Outcome\n def value! = raise UnwrapError, self\nend\n",
            "value!",
        ),
        (
            "Main.scala",
            "object Main { val choose = [X] => (_: List[X]).headOption }",
            "Main",
        ),
        (
            "main.lisp",
            "(defun target () \"~* ~S ~~ ~unknown ~%\")",
            "target",
        ),
        (
            "main.swift",
            "struct Carrier {\n #if FLAG\n #error(\"diagnostic\")\n #endif\n func render() { consume(send: ()) }\n}\n",
            "render",
        ),
        (
            "Widget.svelte",
            "<script>function clicked() {}</script><button on:click={clicked}>Go</button>",
            "clicked",
        ),
        (
            "Page.astro",
            "---\nfunction render() { return 'hello' }\n---\n<h1>{render()}</h1>",
            "render",
        ),
        (
            "Page.ejs",
            "<% function render() { return 'hello' } %><p><%= render() %></p>",
            "render",
        ),
        (
            "Page.cshtml",
            "@functions { public string Render() { return \"hello\"; } }<p>@Render()</p>",
            "Render",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        let report = p.build(false)?;
        if report.coverage.failed_files > 0 {
            failures.push(format!("rejected {file}"));
        }
        let rows = p.query(Query::Symbols { file: file.into() })?;
        if !rows
            .rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == expected))
        {
            failures.push(format!("missing {expected}: {file}"));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
    Ok(())
}

#[test]
fn type_only_star_exports_and_shift_lexical_boundaries_are_preserved() -> TestResult {
    let p = Project::new()?;
    p.write("types.ts", "export interface Value {}")?;
    p.write("index.ts", "export type * from './types';")?;
    assert_eq!(p.build(false)?.coverage.failed_files, 0);
    p.write("invalid.ts", "const value = left < < right;")?;
    assert_eq!(p.build(false)?.coverage.partial_files, 1);
    p.write(
        "invalid.ts",
        "interface Bad<E> { <K>(key: K): E[K] <T>(key: T): E[T] }",
    )?;
    assert_eq!(p.build(false)?.coverage.failed_files, 1);
    Ok(())
}

#[test]
fn erlang_token_fragments_keep_macro_names_and_report_the_missing_expression_context() -> TestResult
{
    let p = Project::new()?;
    p.write("macros.hrl", "-define(WITH_STACKTRACE(T, R, S), T:R:S ->).\n-define(WITH_PEER(Name), case peer:start_link(Name) of {ok, Pid} -> Pid;).\n")?;
    let report = p.build(false)?;
    assert_eq!(report.coverage.failed_files, 0);
    assert_eq!(report.coverage.fragment_files, 1);
    let rows = p.query(Query::Symbols {
        file: "macros.hrl".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name.contains("WITH_STACKTRACE")))
    );
    assert!(!rows.summary.ok_or("summary")?.complete);
    p.write("macros.hrl", "-define(BROKEN, (missing).")?;
    assert_eq!(p.build(false)?.coverage.partial_files, 1);
    Ok(())
}
