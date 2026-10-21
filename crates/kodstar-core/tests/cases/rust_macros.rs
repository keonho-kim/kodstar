// Purpose: Rust macro_export·textual/path scope·재수출·증분 경계 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/application/analysis/resolve/rust/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn exported_macros_are_paths_at_the_crate_root_and_root_lexical_fallback() -> TestResult {
    let p = Project::new()?;
    p.write(
        "src/lib.rs",
        "mod api; fn caller(){ crate::global!(); global!(); }",
    )?;
    p.write(
        "src/api.rs",
        "#[macro_export] macro_rules! global { () => {} }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("src/lib.rs", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(
        calls
            .iter()
            .filter(|e| e.resolution == "resolved"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.name == "global" && t.file == "src/api.rs"))
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn textual_macros_do_not_gain_a_qualified_path_from_their_physical_owner() -> TestResult {
    let p = Project::new()?;
    p.write("src/lib.rs", "mod api { macro_rules! hidden { () => {} } fn inside(){ hidden!(); } #[macro_export] macro_rules! global { () => {} } } fn caller(){ crate::api::hidden!(); crate::api::global!(); crate::global!(); }")?;
    p.build(false)?;
    let inside = p.query(Query::Deps {
        selector: selector("src/lib.rs", "api::inside"),
    })?;
    assert!(
        relationships(&inside)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "hidden"))
    );
    let rows = p.query(Query::Explain {
        selector: selector("src/lib.rs", "caller"),
    })?;
    for expression in ["crate::api::hidden!", "crate::api::global!"] {
        assert!(
            !relationships(&rows)
                .iter()
                .any(|e| e.expression == expression && e.target.is_some()),
            "{:?}",
            rows.rows
        );
    }
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "crate::global!" && e.resolution == "resolved")
    );
    Ok(())
}

#[test]
fn explicit_macro_reexports_make_a_path_without_exporting_the_textual_name() -> TestResult {
    let p = Project::new()?;
    p.write("src/lib.rs", "mod api { macro_rules! helper { () => {} } pub(crate) use helper as exposed; } fn caller(){ crate::api::exposed!(); }")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("src/lib.rs", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "api::helper")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn duplicate_exported_macro_names_are_ambiguous_and_never_first_match() -> TestResult {
    let p = Project::new()?;
    p.write(
        "src/lib.rs",
        "mod one; mod two; fn caller(){ crate::shared!(); }",
    )?;
    p.write(
        "src/one.rs",
        "#[macro_export] macro_rules! shared { () => {} }",
    )?;
    p.write(
        "src/two.rs",
        "#[macro_export] macro_rules! shared { () => {} }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("src/lib.rs", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.resolution == "ambiguous"),
        "{:?}",
        rows.rows
    );
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.resolution == "resolved")
    );
    Ok(())
}

#[test]
fn conditional_exports_remain_candidates_and_new_exports_invalidate_callers() -> TestResult {
    let p = Project::new()?;
    p.write("src/lib.rs", "mod api; fn caller(){ crate::global!(); }")?;
    p.write("src/api.rs", "macro_rules! global { () => {} }")?;
    p.build(false)?;
    let query = || Query::Explain {
        selector: selector("src/lib.rs", "caller"),
    };
    let before = p.query(query())?;
    assert!(!relationships(&before).iter().any(|e| e.target.is_some()));
    p.write(
        "src/api.rs",
        "#[cfg(feature=\"enabled\")] #[macro_export] macro_rules! global { () => {} }",
    )?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    let rows = p.query(query())?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.resolution == "candidate"
                && e.target.as_ref().is_some_and(|t| t.name == "global")),
        "{:?}",
        rows.rows
    );
    let signatures = |rows: &Collected| {
        relationships(rows)
            .iter()
            .map(|e| {
                (
                    e.resolution.clone(),
                    e.target.as_ref().map(|t| t.logical_key.clone()),
                )
            })
            .collect::<Vec<_>>()
    };
    p.build(true)?;
    assert_eq!(signatures(&rows), signatures(&p.query(query())?));
    Ok(())
}

#[test]
fn root_exported_macro_import_aliases_resolve_without_a_bang_in_the_use_tree() -> TestResult {
    let p = Project::new()?;
    p.write(
        "src/lib.rs",
        "mod api; use crate::global as alias; fn caller(){ alias!(); }",
    )?;
    p.write(
        "src/api.rs",
        "#[macro_export] macro_rules! global { () => {} }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("src/lib.rs", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.name == "global" && t.file == "src/api.rs")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn macro_exports_do_not_cross_indexed_cargo_package_boundaries() -> TestResult {
    let p = Project::new()?;
    p.write("Cargo.toml", "[package]\nname='main'\nversion='0.1.0'\n")?;
    p.write("src/lib.rs", "fn caller(){ crate::global!(); }")?;
    p.write(
        "other/Cargo.toml",
        "[package]\nname='other'\nversion='0.1.0'\n",
    )?;
    p.write(
        "other/src/lib.rs",
        "#[macro_export] macro_rules! global { () => {} }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("src/lib.rs", "caller"),
    })?;
    assert!(
        !relationships(&rows).iter().any(|e| e.target.is_some()),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn path_based_exports_allow_forward_root_calls_while_textual_names_keep_order() -> TestResult {
    let p = Project::new()?;
    p.write("src/lib.rs", "fn caller(){ entry!(); local!(); crate::local!(); } #[macro_export] macro_rules! entry { () => {} } macro_rules! local { () => {} }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("src/lib.rs", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "entry!" && e.resolution == "resolved")
    );
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "local")),
        "{:?}",
        rows.rows
    );
    Ok(())
}
