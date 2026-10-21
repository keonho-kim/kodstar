// Purpose: Rust 타입·값·macro 이름 공간과 생성자·textual scope 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn rust_same_named_type_and_value_resolve_in_their_own_namespaces() -> TestResult {
    let p = Project::new()?;
    p.write(
        "lib.rs",
        "struct Item { value: i32 }\nfn Item() {}\nfn caller(value: Item) { Item(); }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("lib.rs", "caller"),
    })?;
    for (relation, kind) in [("calls", "function"), ("uses_type", "struct")] {
        let matches: Vec<_> = relationships(&rows)
            .into_iter()
            .filter(|e| e.relation == relation)
            .collect();
        assert_eq!(matches.len(), 1, "{:?}", rows.rows);
        assert_eq!(matches[0].resolution, "resolved");
        assert!(matches[0].target.as_ref().is_some_and(|t| t.kind == kind));
    }
    Ok(())
}

#[test]
fn rust_one_import_can_expose_distinct_type_and_value_items() -> TestResult {
    let p = Project::new()?;
    p.write("lib.rs","mod api { pub struct Item { value: i32 } pub fn Item() {} }\nuse crate::api::Item;\nfn caller(value: Item) { Item(); }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("lib.rs", "caller"),
    })?;
    let edges = relationships(&rows);
    assert!(
        edges.iter().any(|e| e.relation == "calls"
            && e.resolution == "resolved"
            && e.target.as_ref().is_some_and(|t| t.kind == "function")),
        "{:?}",
        rows.rows
    );
    assert!(edges.iter().any(|e| e.relation == "uses_type"
        && e.resolution == "resolved"
        && e.target.as_ref().is_some_and(|t| t.kind == "struct")));
    Ok(())
}

#[test]
fn rust_tuple_constructors_do_not_make_braced_structs_callable() -> TestResult {
    let p = Project::new()?;
    p.write(
        "lib.rs",
        "struct Tuple(i32);\nstruct Plain { value: i32 }\nfn caller() { Tuple(1); Plain(); }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("lib.rs", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "Tuple" && e.resolution == "resolved")
    );
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "Plain" && e.resolution == "unresolved")
    );
    Ok(())
}

#[test]
fn rust_imported_bang_macros_and_textual_definitions_keep_macro_identity() -> TestResult {
    let p = Project::new()?;
    p.write("lib.rs","fn before() { later!(); }\nmacro_rules! later { () => {} }\n#[macro_export]\nmacro_rules! target { () => {} }\nfn target() {}\nuse crate::target as renamed;\nfn caller() { renamed!(); target(); later!(); }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("lib.rs", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 3, "{:?}", rows.rows);
    assert!(calls.iter().all(|e| e.resolution == "resolved"));
    assert_eq!(
        calls
            .iter()
            .filter(|e| e.target.as_ref().is_some_and(|t| t.kind == "macro"))
            .count(),
        2
    );
    let before = p.query(Query::Explain {
        selector: selector("lib.rs", "before"),
    })?;
    assert!(
        relationships(&before)
            .iter()
            .all(|e| e.resolution == "unresolved")
    );
    Ok(())
}

#[test]
fn rust_self_constructors_and_typed_fields_use_distinct_type_and_value_lookups() -> TestResult {
    let p = Project::new()?;
    p.write("lib.rs","struct Item { value: i32 }\nfn Item() {}\nimpl Item { fn target(&self) {} }\nstruct Owner { item: Item }\nimpl Owner { fn caller(&self) { self.item.target(); } }\nstruct Tuple(i32);\nimpl Tuple { fn create() -> Self { Self(1) } }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("lib.rs", "Owner::caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Item::target")),
        "{:?}",
        rows.rows
    );
    let create = p.query(Query::Deps {
        selector: selector("lib.rs", "Tuple::create"),
    })?;
    assert!(
        relationships(&create)
            .iter()
            .any(|e| e.relation == "calls" && e.target.as_ref().is_some_and(|t| t.name == "Tuple")),
        "{:?}",
        create.rows
    );
    Ok(())
}

#[test]
fn rust_imports_can_walk_type_items_without_confusing_same_named_functions() -> TestResult {
    let p = Project::new()?;
    p.write("lib.rs","mod api { pub struct Model {} pub fn Model() {} impl Model { pub fn run() {} } pub enum Choice { Tuple(i32), Unit } }\nuse crate::api::{Model, Choice::Tuple};\nfn caller() { Model::run(); Tuple(1); }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("lib.rs", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 2, "{:?}", rows.rows);
    assert!(calls.iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "api::Model::run")
    }));
    assert!(calls.iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "api::Choice::Tuple")
    }));
    Ok(())
}

#[test]
fn rust_raw_identifiers_share_keys_while_preserving_original_names() -> TestResult {
    let p = Project::new()?;
    p.write("src/lib.rs","mod r#api; use crate::r#api::r#target as invoke; fn caller() { invoke(); api::target(); api::exposed(); } struct Owner; impl Owner { fn r#method() {} fn call() { Owner::method(); } }")?;
    p.write(
        "src/api.rs",
        "pub fn r#target() {} pub use self::target as r#exposed;",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("src/lib.rs", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.name == "r#target"))
            .map(|e| e.count)
            .sum::<u64>(),
        3,
        "{:?}",
        rows.rows
    );
    let method = p.query(Query::Deps {
        selector: selector("src/lib.rs", "Owner::call"),
    })?;
    assert!(
        relationships(&method)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "r#method")),
        "{:?}",
        method.rows
    );
    for name in ["target", "r#target"] {
        let found = p.query(Query::Search {
            name: name.into(),
            prefix: false,
        })?;
        assert!(
            found
                .rows
                .iter()
                .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="r#target")),
            "{:?}",
            found.rows
        );
    }
    Ok(())
}
