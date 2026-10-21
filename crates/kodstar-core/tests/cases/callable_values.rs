// Purpose: Python·JS/TS의 함수 값 참조·잠재 호출과 shadowing 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn named_callback_arguments_reference_functions_without_establishing_direct_calls() -> TestResult {
    for (file, source) in [
        (
            "main.py",
            "def handler(): pass\ndef submit(value): pass\ndef caller(): submit(handler)\n",
        ),
        (
            "main.js",
            "function handler(){} function submit(value){} function caller(){ submit(handler); }",
        ),
        (
            "main.ts",
            "function handler(){} function submit(value: unknown){} function caller(){ submit(handler); }",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector(file, "caller"),
        })?;
        let handler = relationships(&rows)
            .into_iter()
            .filter(|e| e.target.as_ref().is_some_and(|t| t.name == "handler"))
            .collect::<Vec<_>>();
        assert!(
            handler
                .iter()
                .any(|e| e.relation == "references" && e.resolution == "resolved"),
            "{:?}",
            rows.rows
        );
        assert!(
            handler
                .iter()
                .any(|e| e.relation == "indirect_call" && e.resolution == "candidate"),
            "{:?}",
            rows.rows
        );
        assert!(!handler.iter().any(|e| e.relation == "calls"));
    }
    Ok(())
}

#[test]
fn parameter_and_data_arguments_do_not_become_same_named_global_callbacks() -> TestResult {
    for (file, source) in [
        (
            "main.py",
            "def handler(): pass\ndef submit(value): pass\ndef caller(handler):\n    value=1\n    submit(handler)\n    submit(value)\n",
        ),
        (
            "main.js",
            "function handler(){} function submit(value){} function caller(handler){const value=1;submit(handler);submit(value);}",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector(file, "caller"),
        })?;
        assert!(
            !relationships(&rows)
                .iter()
                .any(|e| e.relation == "indirect_call"
                    || e.relation == "references"
                        && e.target.as_ref().is_some_and(|t| t.name == "handler")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn function_values_in_dispatch_collections_use_values_and_ignore_keys_and_classes() -> TestResult {
    for (file, source) in [
        (
            "main.py",
            "def handler(): pass\ndef key(): pass\nclass Factory: pass\ndef caller():\n    table={key: handler}\n    callbacks=[handler,Factory]\n",
        ),
        (
            "main.js",
            "function handler(){} function key(){} class Factory{} function caller(){const table={key:handler};const callbacks=[handler,Factory];}",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector(file, "caller"),
        })?;
        let indirect = relationships(&rows)
            .into_iter()
            .filter(|e| e.relation == "indirect_call")
            .collect::<Vec<_>>();
        assert_eq!(
            indirect.iter().map(|e| e.count).sum::<u64>(),
            2,
            "{:?}",
            rows.rows
        );
        assert!(
            indirect
                .iter()
                .all(|e| e.target.as_ref().is_some_and(|t| t.name == "handler"))
        );
    }
    Ok(())
}

#[test]
fn imported_callback_values_keep_their_module_and_default_impact_follows_the_reference()
-> TestResult {
    let p = Project::new()?;
    p.write("handler.js", "export function handler(){}")?;
    p.write("main.js","import {handler as callback} from './handler.js'; function submit(value){} function caller(){submit(callback);}")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.js", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.file == "handler.js" && t.name == "handler")),
        "{:?}",
        rows.rows
    );
    let impact = p.query(Query::Impact {
        file: "handler.js".into(),
        name: None,
        at: None,
    })?;
    assert!(
        relationships(&impact)
            .iter()
            .any(|e| e.relation == "references" && e.source.qualified_name == "caller"),
        "{:?}",
        impact.rows
    );
    Ok(())
}

#[test]
fn keyword_callback_arguments_follow_established_local_aliases() -> TestResult {
    let p = Project::new()?;
    p.write("main.py","def handler(): pass\ndef submit(value): pass\ndef caller():\n    callback=handler\n    submit(value=callback)\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.py", "caller"),
    })?;
    let edges = relationships(&rows)
        .into_iter()
        .filter(|e| e.target.as_ref().is_some_and(|t| t.name == "handler"))
        .collect::<Vec<_>>();
    assert_eq!(
        edges
            .iter()
            .filter(|e| e.relation == "references")
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    assert_eq!(
        edges
            .iter()
            .filter(|e| e.relation == "indirect_call")
            .map(|e| e.count)
            .sum::<u64>(),
        2
    );
    Ok(())
}

#[test]
fn external_imports_do_not_capture_a_same_named_indexed_callback() -> TestResult {
    let p = Project::new()?;
    p.write("unrelated.js", "export function handler(){}")?;
    p.write("main.js","import {handler} from 'not-indexed-package'; function submit(value){} function caller(){submit(handler);}")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.js", "caller"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "references" || e.relation == "indirect_call"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn module_callback_registration_and_object_shorthand_keep_real_use_sites() -> TestResult {
    let p = Project::new()?;
    p.write("handler.js", "export function handler(){}")?;
    p.write(
        "main.js",
        "import {handler} from './handler.js'; register(handler); const table={handler};",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Refs {
        selector: selector("handler.js", "handler"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "references"
                && e.source.kind == "file"
                && e.source.file == "main.js")
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn typescript_type_only_function_imports_do_not_establish_runtime_callback_values() -> TestResult {
    let p = Project::new()?;
    p.write("handler.ts", "export function handler(){}")?;
    p.write("main.ts","import type {handler} from './handler.ts'; function submit(value: unknown){} function caller(){submit(handler);}")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.ts", "caller"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "references" || e.relation == "indirect_call"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn typescript_type_only_reexports_do_not_establish_runtime_callback_values() -> TestResult {
    let p = Project::new()?;
    p.write("handler.ts", "export function handler(){}")?;
    p.write(
        "api.ts",
        "export type {handler as callback} from './handler.ts';",
    )?;
    p.write("main.ts","import {callback} from './api.ts'; function submit(value: unknown){} function caller(){submit(callback);}")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.ts", "caller"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "references" || e.relation == "indirect_call"),
        "{:?}",
        rows.rows
    );
    Ok(())
}
