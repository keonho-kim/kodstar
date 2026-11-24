// Purpose: 언어 공통의 본문 재해석에서 무관한 관계와 전체 분석 결과 보존 검증.
// Pattern: Differential Regression Test.
// Usage: engine 통합 검사가 원래 구문 환경이 같은 호출 변경을 적용.
// Related: crates/kodstar-core/src/application/analysis/incremental.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{
    packed_mutations::{mark_relationship_ids, relationship_ids},
    snapshot::normalized_index,
    *,
};

#[test]
fn a_longer_go_call_updates_following_declarations_without_wider_resolution() -> TestResult {
    let p = Project::new()?;
    let source = "package api\nfunc caller() { one(); }\nfunc one() {}\nfunc longer() {}\n";
    p.write("api.go", source)?;
    p.write("stable.py", "def keeper(): pass\ndef stable(): keeper()\n")?;
    let initial = p.build(false)?;
    let expected = mark_relationship_ids(&initial, "stable.py")?;
    p.write("api.go", &source.replace("one();", "longer();"))?;
    let changed = p.build(false)?;
    assert_eq!(relationship_ids(&changed, "stable.py")?, expected);
    assert_eq!(
        normalized_index(&changed)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn stable_body_environments_preserve_other_languages_and_match_full() -> TestResult {
    for (file, source) in [
        (
            "api.py",
            "def alpha(): pass\ndef beta(): pass\ndef caller(): alpha()\n",
        ),
        (
            "api.js",
            "function alpha() {} function beta() {} function caller() { alpha(); }",
        ),
        (
            "api.ts",
            "function alpha() {} function beta() {} function caller() { alpha(); }",
        ),
        (
            "api.rs",
            "fn alpha() {} fn beta() {} fn caller() { alpha(); }",
        ),
        (
            "api.c",
            "void alpha() {} void beta() {} void caller() { alpha(); }",
        ),
        (
            "api.cpp",
            "void alpha() {} void beta() {} void caller() { alpha(); }",
        ),
        (
            "api.go",
            "package api\nfunc alpha() {}\nfunc beta() {}\nfunc caller() { alpha(); }\n",
        ),
        (
            "Api.java",
            "class Api { void alpha() {} void beta() {} void caller() { alpha(); } }",
        ),
        (
            "Api.cs",
            "class Api { void alpha() {} void beta() {} void caller() { alpha(); } }",
        ),
        (
            "api.kt",
            "fun alpha() {}\nfun beta() {}\nfun caller() { alpha() }\n",
        ),
        (
            "api.swift",
            "func alpha() {}\nfunc beta() {}\nfunc caller() { alpha() }\n",
        ),
        (
            "api.rb",
            "def alpha; end\ndef beta; end\ndef caller; alpha(); end\n",
        ),
        (
            "api.php",
            "<?php function alpha() {} function beta() {} function caller() { alpha(); }",
        ),
        (
            "api.zig",
            "fn alpha() void {}\nfn beta() void {}\nfn caller() void { alpha(); }\n",
        ),
        (
            "api.lisp",
            "(defun alpha () nil)\n(defun beta () nil)\n(defun caller () (alpha))\n",
        ),
        (
            "Api.scala",
            "object Api { def alpha(): Unit = {}\ndef beta(): Unit = {}\ndef caller(): Unit = { alpha() } }",
        ),
        (
            "api.ex",
            "defmodule Api do\n def alpha(), do: :ok\n def beta(), do: :ok\n def caller(), do: alpha()\nend\n",
        ),
        (
            "api.erl",
            "-module(api).\nalpha() -> ok.\nbeta() -> ok.\ncaller() -> alpha().\n",
        ),
    ] {
        let source = source.replace("beta", "bravo");
        let p = Project::new()?;
        p.write(file, &source)?;
        p.write("stable.py", "def keeper(): pass\ndef stable(): keeper()\n")?;
        let initial = p.build(false)?;
        let expected = mark_relationship_ids(&initial, "stable.py")?;
        let changed_source = source
            .replace("alpha();", "bravo();")
            .replace("alpha() }", "bravo() }")
            .replace("caller(): alpha()", "caller(): bravo()")
            .replace("do: alpha()", "do: bravo()")
            .replace("-> alpha().", "-> bravo().")
            .replace(" () (alpha))", " () (bravo))");
        assert_ne!(source, changed_source, "fixture did not change: {file}");
        p.write(file, &changed_source)?;
        let changed = p.build(false)?;
        assert_eq!(
            relationship_ids(&changed, "stable.py")?,
            expected,
            "unrelated relationships rewritten: {file}"
        );
        assert_eq!(
            normalized_index(&changed)?,
            normalized_index(&p.build(true)?)?,
            "{file}"
        );
    }
    Ok(())
}
