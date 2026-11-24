// Purpose: Java·C# 생성자·명시적 반환 타입의 receiver 연결 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn java_and_csharp_constructor_and_annotated_return_values_establish_receivers() -> TestResult {
    for (file, source, caller, target) in [
        (
            "Main.java",
            "class Item { void run() {} } class Factory { static Item make() { return new Item(); } void caller() { var value=make(); value.run(); make().run(); new Item().run(); } }",
            "Factory::caller",
            "Item::run",
        ),
        (
            "Main.cs",
            "class Item { public void Run() {} } class Factory { static Item Make() { return new Item(); } void caller() { var value=Make(); value.Run(); Make().Run(); new Item().Run(); } }",
            "Factory::caller",
            "Item::Run",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Deps {
            selector: selector(file, caller),
        })?;
        assert_eq!(
            relationships(&rows)
                .iter()
                .filter(|e| e.relation == "calls"
                    && e.target
                        .as_ref()
                        .is_some_and(|t| t.qualified_name == target))
                .map(|e| e.count)
                .sum::<u64>(),
            3,
            "{file}: {:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn java_return_annotation_changes_invalidate_unmodified_callers() -> TestResult {
    let p = Project::new()?;
    p.write("Types.java","class First { void run() {} } class Second { void run() {} } class Factory { static First make() { return new First(); } }")?;
    p.write(
        "Main.java",
        "class Main { void caller() { Factory.make().run(); } }",
    )?;
    p.build(false)?;
    let query = Query::Deps {
        selector: selector("Main.java", "Main::caller"),
    };
    assert!(relationships(&p.query(query.clone())?).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "First::run")
    }));
    p.write("Types.java","class First { void run() {} } class Second { void run() {} } class Factory { static Second make() { return new Second(); } }")?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    assert!(relationships(&p.query(query.clone())?).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Second::run")
    }));
    p.build(true)?;
    assert!(relationships(&p.query(query)?).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Second::run")
    }));
    Ok(())
}

#[test]
fn java_argument_count_selects_the_overload_before_resolving_return_members() -> TestResult {
    let p = Project::new()?;
    p.write("Main.java","class Item { void run() {} } class Factory { static Item make() { return new Item(); } static Object make(int value) { return null; } void caller() { make(1).run(); } void valid() { make().run(); } }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.java", "Factory::caller"),
    })?;
    assert!(
        !relationships(&rows).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Item::run")),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows).iter().any(|e| {
            e.resolution == "resolved"
                && e.target.as_ref().is_some_and(|t| {
                    t.qualified_name == "Factory::make" && t.signature.contains("value:int")
                })
        }),
        "{:?}",
        rows.rows
    );
    let valid = p.query(Query::Explain {
        selector: selector("Main.java", "Factory::valid"),
    })?;
    assert!(
        relationships(&valid).iter().any(|e| {
            e.resolution == "resolved"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Item::run")
        }),
        "{:?}",
        valid.rows
    );
    Ok(())
}

#[test]
fn java_same_arity_and_applicable_varargs_remain_ambiguous() -> TestResult {
    let p = Project::new()?;
    p.write("Main.java", "class X { void run(int v) {} void run(String v) {} void run(int... v) {} void caller() { run(1); } }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.java", "X::caller"),
    })?;
    let alternatives: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| {
            e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "X::run")
        })
        .collect();
    assert_eq!(alternatives.len(), 3, "{:?}", rows.rows);
    assert!(alternatives.iter().all(|e| e.resolution == "ambiguous"));
    Ok(())
}
