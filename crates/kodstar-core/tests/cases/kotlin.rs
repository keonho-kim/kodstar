// Purpose: Kotlin package·object/companion·반환 binding과 선택적 구분자 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn kotlin_local_object_values_do_not_shadow_the_type_namespace() -> TestResult {
    let p = Project::new()?;
    p.write("Main.kt","class Item { fun run() {} }\nfun make(): Item = Item()\nfun caller(){ val Item=object { fun run() {} }; val value: Item=make(); value.run() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.kt", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "value.run"
                && e.resolution == "resolved"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Item::run")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn kotlin_object_values_are_not_assumed_to_be_class_constructors() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.kt",
        "object Service { fun run() {} }\nfun caller(){ Service().run() }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.kt", "caller"),
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
fn kotlin_anonymous_object_methods_own_their_calls_and_known_this_receiver() -> TestResult {
    let p = Project::new()?;
    p.write("Main.kt","interface Service { fun run() }\nfun target() {}\nfun factory(){ val handler=object : Service { override fun run(){ target() } fun call(){ this.run() } }; handler.run() }\n")?;
    p.build(false)?;
    let symbols = p.query(Query::Symbols {
        file: "Main.kt".into(),
    })?;
    let run = symbols
        .rows
        .iter()
        .find_map(|r| match r {
            QueryRow::Symbol(s) if s.qualified_name.contains("<object@") && s.name == "run" => {
                Some(s.qualified_name.clone())
            }
            _ => None,
        })
        .ok_or("missing object method")?;
    let caller = run.trim_end_matches("run").to_string() + "call";
    let body = p.query(Query::Deps {
        selector: selector("Main.kt", &run),
    })?;
    assert!(relationships(&body).iter().any(|e|e.relation=="calls" && e.target.as_ref().is_some_and(|t|t.name=="target")),"{:?}",body.rows);
    let own = p.query(Query::Deps {
        selector: selector("Main.kt", &caller),
    })?;
    assert!(
        relationships(&own)
            .iter()
            .any(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.qualified_name == run)),
        "{:?}",
        own.rows
    );
    let factory = p.query(Query::Deps {
        selector: selector("Main.kt", "factory"),
    })?;
    assert!(
        relationships(&factory)
            .iter()
            .any(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.qualified_name == run)),
        "{:?}",
        factory.rows
    );
    assert!(
        !relationships(&factory)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "target")),
        "{:?}",
        factory.rows
    );
    Ok(())
}

#[test]
fn kotlin_signatures_preserve_names_types_and_returns_without_default_literals() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.kt",
        "class Item\nfun make(value: Item, text: String=\"fixture-secret\"): Item = value\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "Main.kt".into(),
    })?;
    let signature = rows
        .rows
        .iter()
        .find_map(|r| match r {
            QueryRow::Symbol(s) if s.name == "make" => Some(&s.signature),
            _ => None,
        })
        .ok_or("signature")?;
    assert!(
        signature.contains("value:Item")
            && signature.contains("text:String")
            && signature.ends_with("Item"),
        "{signature}"
    );
    assert!(!signature.contains("fixture-secret"));
    Ok(())
}

#[test]
fn kotlin_local_receivers_shadow_package_paths_and_malformed_members_still_fail() -> TestResult {
    let p = Project::new()?;
    p.write("Api.kt", "package example.api\nfun target() {}\n")?;
    p.write(
        "Main.kt",
        "fun caller(example: Unknown) { example.api.target() }\n",
    )?;
    let initial = p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.kt", "caller"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "calls" && e.target.is_some()),
        "{:?}",
        rows.rows
    );
    p.write("Main.kt", "class Item { fun broken( { } }")?;
    assert!(p.build(false).is_err());
    let stale = p.query(Query::Symbols {
        file: "Main.kt".into(),
    })?;
    assert_eq!(
        stale.metadata.ok_or("metadata")?.generation,
        Some(initial.generation)
    );
    Ok(())
}

#[test]
fn kotlin_optional_class_separators_do_not_reject_complete_member_syntax() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.kt",
        "class Item { fun target() {} fun caller() { target() } }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.kt", "Item::caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Item::target")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn kotlin_package_qualified_calls_resolve_from_the_declared_package() -> TestResult {
    let p = Project::new()?;
    p.write("Api.kt","package example.api\nfun target() {}\nobject Service {\n fun run() {}\n}\nclass Holder {\n companion object {\n fun make() {}\n }\n}\n")?;
    p.write("Main.kt","package app\nimport example.api.Service as Api\nfun caller() { example.api.target(); Api.run(); example.api.Holder.make() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.kt", "caller"),
    })?;
    for name in ["target", "Service::run", "Holder::make"] {
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.resolution == "resolved"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.file == "Api.kt" && t.qualified_name == name)),
            "{name}: {:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn kotlin_constructor_and_return_values_establish_local_receiver_types() -> TestResult {
    let p = Project::new()?;
    p.write("Main.kt","class Item {\n fun run() {}\n}\nfun make(): Item = Item()\nfun caller(){ val value: Item=make(); val inferred=Item(); value.run(); inferred.run(); make().run() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.kt", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Item::run"))
            .map(|e| e.count)
            .sum::<u64>(),
        3,
        "{:?}",
        rows.rows
    );
    Ok(())
}
