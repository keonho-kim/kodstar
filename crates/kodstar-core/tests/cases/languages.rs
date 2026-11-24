// Purpose: 선택한 언어별 정의·호출 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn every_selected_language_extracts_real_definitions_and_calls() -> TestResult {
    let cases = [
        (
            "sample.py",
            "def target():\n    pass\ndef caller():\n    target()\n",
            "target",
            "caller",
        ),
        (
            "sample.js",
            "export function target() {}\nexport function caller() { target(); }\n",
            "target",
            "caller",
        ),
        (
            "sample.ts",
            "export function target(): void {}\nexport function caller(): void { target(); }\n",
            "target",
            "caller",
        ),
        (
            "sample.rs",
            "pub fn target() {}\npub fn caller() { target(); }\n",
            "target",
            "caller",
        ),
        (
            "sample.go",
            "package sample\nfunc target() {}\nfunc caller() { target() }\n",
            "target",
            "caller",
        ),
        (
            "Sample.java",
            "class Sample { static void target() {} static void caller() { target(); } }",
            "target",
            "Sample::caller",
        ),
        (
            "sample.c",
            "void target(void) {}\nvoid caller(void) { target(); }",
            "target",
            "caller",
        ),
        (
            "sample.cpp",
            "void target() {}\nvoid caller() { target(); }",
            "target",
            "caller",
        ),
        (
            "Sample.cs",
            "class Sample { static void target() {} static void caller() { target(); } }",
            "target",
            "Sample::caller",
        ),
        (
            "sample.php",
            "<?php function target() {} function caller() { target(); }",
            "target",
            "caller",
        ),
        (
            "sample.kt",
            "fun target() {}\nfun caller() { target() }",
            "target",
            "caller",
        ),
        (
            "sample.swift",
            "func target() {}\nfunc caller() { target() }",
            "target",
            "caller",
        ),
        (
            "sample.rb",
            "def target\nend\ndef caller\n  target()\nend\n",
            "target",
            "caller",
        ),
        (
            "sample.scala",
            "object Sample { def target(): Unit = {}\ndef caller(): Unit = { target() } }",
            "target",
            "Sample::caller",
        ),
        (
            "sample.zig",
            "fn target() void {}\nfn caller() void { target(); }",
            "target",
            "caller",
        ),
        (
            "sample.ex",
            "defmodule Sample do\n  def target(), do: :ok\n  def caller(), do: target()\nend\n",
            "target",
            "Sample::caller",
        ),
        (
            "sample.erl",
            "-module(sample).\n-export([target/0, caller/0]).\ntarget() -> ok.\ncaller() -> target().\n",
            "target/0",
            "caller/0",
        ),
        (
            "sample.lisp",
            "(defun target () nil)\n(defun caller () (target))\n",
            "target",
            "caller",
        ),
    ];
    assert_eq!(
        cases.len(),
        Language::ALL
            .iter()
            .filter(|language| !language.is_component())
            .count()
    );
    for (file, source, target, caller) in cases {
        let project = Project::new()?;
        project.write(file, source)?;
        project
            .build(false)
            .map_err(|error| format!("{file}: {error}"))?;
        let result = project
            .query(Query::Deps {
                selector: selector(file, caller),
            })
            .map_err(|error| format!("{file}: {error}"))?;
        let calls: Vec<_> = relationships(&result)
            .into_iter()
            .filter(|edge| edge.relation == "calls")
            .collect();
        assert_eq!(calls.len(), 1, "{file}: {calls:?}");
        assert_eq!(
            calls[0].target.as_ref().map(|symbol| symbol.name.as_str()),
            Some(target),
            "{file}: {calls:?}"
        );
    }
    Ok(())
}
