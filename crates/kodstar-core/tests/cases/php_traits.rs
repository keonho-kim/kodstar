// Purpose: PHP trait 적응·접근 제한·증분 무효화 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn php_trait_selection_and_alias_keep_the_original_definition() -> TestResult {
    let p = Project::new()?;
    p.write("main.php",r"<?php trait First { public function Run() {} } trait Second { public function Run() {} } class Item { use First, Second { First::Run insteadof Second; Second::Run as Alternate; } } function caller(Item $value) { $value->run(); $value->alternate(); }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.php", "caller"),
    })?;
    for (expression, target) in [
        ("$value::run", "First::Run"),
        ("$value::alternate", "Second::Run"),
    ] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.expression == expression
                    && e.resolution == "resolved"
                    && e.target
                        .as_ref()
                        .is_some_and(|t| t.qualified_name == target)),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn php_private_trait_alias_and_methods_are_not_externally_callable() -> TestResult {
    let p = Project::new()?;
    p.write("main.php",r"<?php trait Feature { public function Run() {} } class Item { use Feature { Run as private hidden; } private function Secret() {} public function inside() { $this->secret(); $this->hidden(); } } function caller(Item $value) { $value->secret(); $value->hidden(); $value->run(); }")?;
    p.build(false)?;
    let outside = p.query(Query::Explain {
        selector: selector("main.php", "caller"),
    })?;
    for expression in ["$value::secret", "$value::hidden"] {
        assert!(
            relationships(&outside)
                .iter()
                .any(|e| e.expression == expression && e.resolution == "unresolved"),
            "{:?}",
            outside.rows
        );
    }
    let inside = p.query(Query::Deps {
        selector: selector("main.php", "Item::inside"),
    })?;
    for target in ["Item::Secret", "Feature::Run"] {
        assert!(
            relationships(&inside).iter().any(|e| e.relation == "calls"
                && e.resolution == "resolved"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == target)),
            "{:?}",
            inside.rows
        );
    }
    Ok(())
}

#[test]
fn php_trait_rule_changes_reresolve_unchanged_callers_and_match_rebuilds() -> TestResult {
    let p = Project::new()?;
    p.write(
        "traits.php",
        r"<?php trait First { function Run() {} } trait Second { function Run() {} }",
    )?;
    p.write(
        "item.php",
        r"<?php class Item { use First, Second { First::Run insteadof Second; } }",
    )?;
    p.write(
        "caller.php",
        r"<?php function caller(Item $value) { $value->run(); }",
    )?;
    p.build(false)?;
    let query = Query::Deps {
        selector: selector("caller.php", "caller"),
    };
    assert!(relationships(&p.query(query.clone())?).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "First::Run")
    }));
    p.write(
        "item.php",
        r"<?php class Item { use First, Second { Second::Run insteadof First; } }",
    )?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    let incremental = p.query(query.clone())?;
    assert!(
        relationships(&incremental).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Second::Run")),
        "{:?}",
        incremental.rows
    );
    p.build(true)?;
    let full = p.query(query)?;
    let names = |rows: &Collected| {
        relationships(rows)
            .into_iter()
            .filter_map(|e| {
                e.target
                    .as_ref()
                    .map(|t| (t.qualified_name.clone(), e.resolution.clone(), e.count))
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&incremental), names(&full));
    Ok(())
}

#[test]
fn php_deep_return_chains_publish_a_file_work_limit() -> TestResult {
    let p = Project::new()?;
    p.write("main.php",r"<?php class Item { function copy(): self { return $this; } } function caller(Item $item) { $item->copy(); }")?;
    let initial = p.build(false)?;
    let chain = "$item".to_string() + &"->copy()".repeat(40);
    p.write(
        "main.php",
        &format!("<?php function caller($item) {{ {chain}; }}"),
    )?;
    assert_eq!(p.build(false)?.coverage.failed_files, 1);
    let rows = p.query(Query::Symbols {
        file: "main.php".into(),
    })?;
    assert_eq!(
        rows.metadata.ok_or("metadata")?.generation,
        Some(initial.generation + 1)
    );
    Ok(())
}
