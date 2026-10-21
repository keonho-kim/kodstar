// Purpose: PHP namespace·import 공간·대소문자와 전역 fallback 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn php_grouped_imports_keep_class_function_and_constant_spaces_separate() -> TestResult {
    let p = Project::new()?;
    p.write(
        "vendor.php",
        r"<?php namespace Vendor; class Thing {} function Thing() {} function Run() {}",
    )?;
    p.write("main.php",r"<?php namespace App; use Vendor\{Thing as Item, function Run as execute}; use function Vendor\Thing as Item; function caller(Item $value) { ITEM(); EXECUTE(); new item(); }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.php", "App::caller"),
    })?;
    for (name, kind) in [("ITEM", "function"), ("EXECUTE", "function")] {
        assert!(
            relationships(&rows).iter().any(|e| e.expression == name
                && e.resolution == "resolved"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.file == "vendor.php" && t.kind == kind)),
            "{:?}",
            rows.rows
        );
    }
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.name == "Thing" && t.kind == "class")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn php_relative_and_absolute_function_paths_do_not_use_global_name_guesses() -> TestResult {
    let p = Project::new()?;
    p.write(
        "global.php",
        r"<?php function GlobalOnly() {} function Target() {}",
    )?;
    p.write(
        "local.php",
        r"<?php namespace App; function Target() {} namespace App\Child; function nested() {}",
    )?;
    p.write("main.php",r"<?php namespace App; use App\Child as Sub; function caller() { TARGET(); GlobalOnly(); \Target(); namespace\TARGET(); Sub\nested(); Missing\Target(); }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.php", "App::caller"),
    })?;
    for (expression, file) in [
        ("TARGET", "local.php"),
        ("GlobalOnly", "global.php"),
        (r"\Target", "global.php"),
        (r"namespace\TARGET", "local.php"),
        (r"Sub\nested", "local.php"),
    ] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.expression == expression
                    && e.resolution == "resolved"
                    && e.target.as_ref().is_some_and(|t| t.file == file)),
            "{expression}: {:?}",
            rows.rows
        );
    }
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == r"Missing\Target" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn php_dynamic_function_variables_do_not_bind_same_named_functions() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.php",
        r"<?php function invoke() {} function caller($invoke) { $invoke(); }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.php", "caller"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "calls" && e.resolution == "resolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn php_new_return_chains_and_method_case_use_established_receiver_types() -> TestResult {
    let p = Project::new()?;
    p.write("main.php",r"<?php namespace App; class Item { public function Run() {} public function copy(): self { return $this; } } function make(): Item { return new Item(); } function caller() { $value=new Item(); $result=make(); $result->run(); make()->RUN(); $value->Run(); Item::run(); $value->copy()->run(); }")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.php", "App::caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "App::Item::Run"))
            .map(|e| e.count)
            .sum::<u64>(),
        5,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn php_constant_imports_are_case_sensitive_and_do_not_shadow_functions() -> TestResult {
    let p = Project::new()?;
    p.write(
        "api.php",
        r"<?php namespace Api; const FLAG=1; const flag=2; function FLAG() {}",
    )?;
    p.write("main.php",r"<?php namespace App; use const Api\FLAG as SELECTED; use function Api\FLAG as SELECTED; function caller() { echo SELECTED; echo selected; SELECTED(); }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.php", "App::caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "SELECTED"
                && e.relation == "references"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.name == "FLAG" && t.kind == "field")),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "selected" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "SELECTED"
                && e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.kind == "function")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn php_class_and_trait_methods_take_precedence_over_parent_methods() -> TestResult {
    let p = Project::new()?;
    p.write("main.php",r"<?php class ParentType { public function Run() {} } trait Feature { public function Run() {} } class WithTrait extends ParentType { use Feature; } class Own extends ParentType { use Feature; public function Run() {} } function caller(WithTrait $a, Own $b) { $a->run(); $b->run(); }")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.php", "caller"),
    })?;
    for target in ["Feature::Run", "Own::Run"] {
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.resolution == "resolved"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == target)),
            "{:?}",
            rows.rows
        );
    }
    assert!(
        !relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "ParentType::Run")),
        "{:?}",
        rows.rows
    );
    Ok(())
}
