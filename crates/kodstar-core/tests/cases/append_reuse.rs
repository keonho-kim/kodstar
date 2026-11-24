// Purpose: 미사용 선언 추가의 관계 재사용과 전체 분석 동등성 검증.
// Pattern: Differential Regression Test.
// Usage: engine 테스트가 명시적으로 표시한 관계 ID와 정규화 결과를 비교.
// Related: crates/kodstar-core/src/infrastructure/index/append_reuse/mod.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{
    packed_mutations::{mark_relationship_ids, relationship_ids},
    snapshot::normalized_index,
    *,
};
use kodstar_core::*;

#[test]
fn unused_appended_declarations_preserve_existing_graph_in_typed_projects() -> TestResult {
    for (file, source, suffix) in [
        (
            "api.go",
            "package api\ntype Service struct {}\nfunc target() {}\nfunc caller() { target() }\n",
            "\nfunc KodstarUnusedAddition() {}\n",
        ),
        (
            "api.py",
            "class Service: pass\ndef target(): pass\ndef caller(): target()\n",
            "\ndef KodstarUnusedAddition(): pass\n",
        ),
        (
            "api.rs",
            "struct Service;\nfn target() {} fn caller() { target(); }\n",
            "\nfn KodstarUnusedAddition() {}\n",
        ),
        (
            "Api.java",
            "package example; class Api { void target() {} void caller() { target(); } }\n",
            "\nclass KodstarUnusedAddition {}\n",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        let first = p.build(false)?;
        assert_eq!(first.coverage.failed_files, 0, "{file}");
        let ids = mark_relationship_ids(&first, file)?;
        p.write(file, &format!("{source}{suffix}"))?;
        let incremental = p.build(false)?;
        assert_eq!(
            relationship_ids(&incremental, file)?,
            ids,
            "unaffected graph recomputed: {file}"
        );
        let actual = normalized_index(&incremental)?;
        let full = p.build(true)?;
        assert_eq!(actual, normalized_index(&full)?, "{file}");
    }
    Ok(())
}

#[test]
fn new_provider_invalidates_failed_lookups_and_scope_changes_fall_back() -> TestResult {
    let p = Project::new()?;
    let before = "class Service: pass\ndef caller(): KodstarAddedProvider()\n";
    p.write("main.py", before)?;
    p.build(false)?;
    p.write(
        "main.py",
        &format!("{before}\ndef KodstarAddedProvider(): pass\n"),
    )?;
    let updated = p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.py", "caller"),
    })?;
    assert!(relationships(&rows).iter().any(|r| {
        r.target
            .as_ref()
            .is_some_and(|s| s.name == "KodstarAddedProvider")
    }));
    let actual = normalized_index(&updated)?;
    assert_eq!(actual, normalized_index(&p.build(true)?)?);
    p.write("main.py", "class Service:\n def caller(self): pass\n")?;
    p.build(false)?;
    p.write(
        "main.py",
        "class Service:\n def caller(self): pass\n def KodstarMember(self): pass\n",
    )?;
    let updated = p.build(false)?;
    let actual = normalized_index(&updated)?;
    assert_eq!(actual, normalized_index(&p.build(true)?)?);
    Ok(())
}

#[test]
fn unused_export_addition_updates_wildcard_consumers_without_changing_old_bodies() -> TestResult {
    for (file, before, after, consumer, source) in [
        (
            "api.py",
            "def original(): pass\n",
            "def original(): pass\ndef KodstarFreshExport(): pass\n",
            "use.py",
            "from api import *\ndef caller(): original()",
        ),
        (
            "api.js",
            "export function original() {}\n",
            "export function original() {}\nexport function KodstarFreshExport() {}\n",
            "entry.js",
            "export * from './api.js';",
        ),
        (
            "Api.java",
            "package api; public class Api {}\n",
            "package api; public class Api {}\nclass KodstarFreshExport {}\n",
            "Use.java",
            "package consumer; import api.*; class Use {}",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, before)?;
        p.write(consumer, source)?;
        p.build(false)?;
        p.write(file, after)?;
        let incremental = p.build(false)?;
        let actual = normalized_index(&incremental)?;
        assert_eq!(actual, normalized_index(&p.build(true)?)?, "{file}");
    }
    Ok(())
}

#[test]
fn isolated_subtrees_keep_old_graph_and_compute_only_their_own_identities() -> TestResult {
    for (file, source, suffix) in [
        (
            "api.ex",
            "defmodule Api do\n def target(), do: :ok\n def caller(), do: target()\nend\n",
            "\ndefmodule KodstarAddition do\n def run(), do: :ok\nend\n",
        ),
        (
            "Api.scala",
            "object Api { def target(): Unit = {}\n def caller(): Unit = { target() } }\n",
            "\nobject KodstarAddition { def run(): Unit = {} }\n",
        ),
        (
            "Api.cs",
            "class Api { static void target() {} static void caller() { target(); } }\n",
            "\nclass KodstarAddition { public void Run() {} }\n",
        ),
        (
            "api.swift",
            "func target() {}\nfunc caller() { target() }\n",
            "\nfunc kodstarAddition() {}\n",
        ),
        (
            "api.erl",
            "-module(api).\n-export([target/0,caller/0]).\ntarget() -> ok.\ncaller() -> target().\n",
            "\nkodstaraddition() -> ok.\n",
        ),
        (
            "api.lisp",
            "(defun target () nil)\n(defun caller () (target))\n",
            "\n(defun kodstar-addition () nil)\n",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        let first = p.build(false)?;
        assert_eq!(first.coverage.failed_files, 0, "{file}");
        let ids = mark_relationship_ids(&first, file)?;
        p.write(file, &format!("{source}{suffix}"))?;
        let changed = p.build(false)?;
        let retained = relationship_ids(&changed, file)?;
        assert!(
            ids.iter().all(|id| retained.contains(id)),
            "graph recomputed: {file}"
        );
        let actual = normalized_index(&changed)?;
        assert_eq!(actual, normalized_index(&p.build(true)?)?, "{file}");
    }
    Ok(())
}

#[test]
fn insertion_before_a_module_end_updates_spans_without_recomputing_old_bodies() -> TestResult {
    for (file, before, after) in [
        (
            "api.ex",
            "defmodule Api do\n def target(), do: :ok\n def caller(), do: target()\nend\n",
            "defmodule Api do\n def target(), do: :ok\n def caller(), do: target()\n def kodstar_addition(), do: :ok\nend\n",
        ),
        (
            "Api.scala",
            "object Api {\n def target(): Unit = {}\n def caller(): Unit = { target() }\n}\n",
            "object Api {\n def target(): Unit = {}\n def caller(): Unit = { target() }\n def kodstarAddition(): Unit = {}\n}\n",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, before)?;
        let first = p.build(false)?;
        let ids = mark_relationship_ids(&first, file)?;
        p.write(file, after)?;
        let changed = p.build(false)?;
        let retained = relationship_ids(&changed, file)?;
        assert!(
            ids.iter().all(|id| retained.contains(id)),
            "old body changed: {file}"
        );
        let actual = normalized_index(&changed)?;
        assert_eq!(actual, normalized_index(&p.build(true)?)?, "{file}");
    }
    Ok(())
}
