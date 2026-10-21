// Purpose: JS/TS 모듈 공개 경계와 정적 바인딩 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn mixed_internal_and_external_reexports_remain_ambiguous() -> TestResult {
    let project = Project::new()?;
    project.write("leaf.ts", "export function target() {}")?;
    project.write("barrel.ts","export { target } from './leaf'; export { target } from 'external-one'; export { target } from 'external-two';")?;
    project.write(
        "main.ts",
        "import { target } from './barrel'; function caller() { target(); }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("main.ts", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|e| e.resolution == "ambiguous"));
    Ok(())
}

#[test]
fn shadowed_commonjs_globals_do_not_establish_module_bindings() -> TestResult {
    let project = Project::new()?;
    project.write("api.js", "exports.target = () => {};")?;
    project.write(
        "main.js",
        "function caller(require) { const api = require('./api'); api.target(); }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.js", "caller"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.file == "api.js"))
    );
    project.write(
        "api.js",
        "const exports = {}; function target() {} exports.target = target;",
    )?;
    project.write(
        "main.js",
        "const api = require('./api'); function caller() { api.target(); }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.js", "caller"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.file == "api.js"))
    );
    Ok(())
}

#[test]
fn anonymous_default_and_commonjs_function_values_are_real_definitions() -> TestResult {
    let project = Project::new()?;
    project.write("default.js", "export default function() {}")?;
    project.write(
        "cjs.js",
        "exports.invoke = () => {}; function target() {} module.exports = { run: target };",
    )?;
    project.write("main.js", "import invoke from './default'; const { invoke: cjs, run } = require('./cjs'); function caller() { invoke(); cjs(); run(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.js", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls" && e.target.is_some())
            .map(|e| e.count)
            .sum::<u64>(),
        3
    );
    Ok(())
}

#[test]
fn changed_local_exports_and_star_cycles_match_a_full_rebuild() -> TestResult {
    let project = Project::new()?;
    project.write(
        "leaf.ts",
        "function first() {} function second() {} export { first as target };",
    )?;
    project.write(
        "barrel.ts",
        "export * from './loop'; export * from './leaf';",
    )?;
    project.write("loop.ts", "export * from './barrel';")?;
    project.write(
        "main.ts",
        "import { target } from './barrel'; function caller() { target(); }",
    )?;
    project.build(false)?;
    let query = || Query::Deps {
        selector: selector("main.ts", "caller"),
    };
    let before = project.query(query())?;
    assert_eq!(
        relationships(&before)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .name,
        "first"
    );
    project.write(
        "leaf.ts",
        "function first() {} function second() {} export { second as target };",
    )?;
    let report = project.build(false)?;
    assert_eq!(report.parsed_files, 1);
    let incremental = project.query(query())?;
    assert_eq!(
        relationships(&incremental)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .name,
        "second"
    );
    project.build(true)?;
    let full = project.query(query())?;
    assert_eq!(
        relationships(&full)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .logical_key,
        relationships(&incremental)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .logical_key
    );
    Ok(())
}

#[test]
fn esm_default_local_star_and_namespace_exports_resolve_without_private_leaks() -> TestResult {
    let project = Project::new()?;
    project.write("leaf.ts", "function hidden() {} function target() {} export { target as renamed }; export default target;")?;
    project.write(
        "barrel.ts",
        "export * from './leaf'; export * as api from './leaf';",
    )?;
    project.write("main.ts", "import invoke from './leaf'; import { renamed, hidden, api } from './barrel'; function caller() { invoke(); renamed(); api.renamed(); hidden(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("main.ts", "caller"),
    })?;
    let edges = relationships(&rows);
    let resolved: u64 = edges
        .iter()
        .filter(|e| {
            e.resolution == "resolved" && e.target.as_ref().is_some_and(|t| t.name == "target")
        })
        .map(|e| e.count)
        .sum();
    assert_eq!(resolved, 3);
    assert!(
        edges
            .iter()
            .any(|e| e.expression == "hidden" && e.resolution == "unresolved")
    );
    Ok(())
}

#[test]
fn commonjs_static_require_destructuring_and_exports_preserve_targets() -> TestResult {
    let project = Project::new()?;
    project.write(
        "api.js",
        "function target() {} exports.invoke = target; module.exports.default = target;",
    )?;
    project.write("main.js", "const api = require('./api'); const { invoke: run } = require('./api'); function caller() { api.invoke(); run(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.js", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.target.as_ref().is_some_and(|t| t.name == "target"))
            .map(|e| e.count)
            .sum::<u64>(),
        2
    );
    Ok(())
}

#[test]
fn typescript_type_only_bindings_never_establish_runtime_calls() -> TestResult {
    let project = Project::new()?;
    project.write(
        "leaf.ts",
        "export class Service {} export function invoke() {}",
    )?;
    project.write(
        "barrel.ts",
        "export type { Service, invoke } from './leaf';",
    )?;
    project.write("main.ts", "import type { Service } from './leaf'; import { invoke } from './barrel'; function caller(value: Service) { invoke(); }")?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("main.ts", "caller"),
    })?;
    assert!(relationships(&rows).iter().any(
        |e| e.relation == "uses_type" && e.target.as_ref().is_some_and(|t| t.name == "Service")
    ));
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "calls" && e.resolution == "resolved")
    );
    let imports = project.query(Query::Explain {
        selector: selector("barrel.ts", "barrel.ts"),
    })?;
    assert!(
        relationships(&imports)
            .iter()
            .any(|e| e.type_only && e.relation == "reexports")
    );
    Ok(())
}

#[test]
fn conditional_commonjs_exports_remain_ambiguous_beside_a_root_assignment() -> TestResult {
    let p = Project::new()?;
    p.write("api.js","function first(){} function second(){} module.exports.handler=first; if(process.env.FLAG){module.exports.handler=second;}")?;
    p.write(
        "main.js",
        "const {handler}=require('./api.js'); function caller(){handler();}",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.js", "caller"),
    })?;
    let calls = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2, "{:?}", rows.rows);
    assert!(calls.iter().all(|e| e.resolution == "ambiguous"));
    Ok(())
}
