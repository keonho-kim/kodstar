// Purpose: Kotlin escaped 이름·AST import·조회 키와 원본 위치 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/domain/resolve/kotlin.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn kotlin_backtick_names_keep_spaces_and_share_identity_with_unquoted_spelling() -> TestResult {
    let p = Project::new()?;
    let source = "fun `target`() {}\nfun `target with space`() {}\nclass `Worker type` {\n fun `run task`() {}\n}\nfun caller(value: `Worker type`) { `target`(); target(); `target with space`(); value.`run task`() }\n";
    p.write("Main.kt", source)?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.kt", "caller"),
    })?;
    let edges = relationships(&rows);
    let calls: Vec<_> = edges.iter().filter(|e| e.relation == "calls").collect();
    assert_eq!(
        calls
            .iter()
            .filter(|e| e.resolution == "resolved")
            .map(|e| e.count)
            .sum::<u64>(),
        4,
        "{:?}",
        rows.rows
    );
    for name in ["`target`", "`target with space`", "`run task`"] {
        assert!(
            calls
                .iter()
                .any(|e| e.target.as_ref().is_some_and(|t| t.name == name)),
            "{:?}",
            rows.rows
        );
    }
    let spaced = calls
        .iter()
        .find(|e| {
            e.target
                .as_ref()
                .is_some_and(|t| t.name == "`target with space`")
        })
        .ok_or("call")?;
    assert_eq!(
        &source[spaced.site.span.start_byte..spaced.site.span.end_byte],
        "`target with space`"
    );
    Ok(())
}

#[test]
fn kotlin_import_aliases_are_ast_names_even_when_they_contain_as() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Api.kt",
        "package `when`.api\nfun `target as name`() {}\nfun plain() {}\n",
    )?;
    p.write("Main.kt", "package app\nimport `when`.api.`target as name` as `name as other`\nimport `when`.api.*\nfun caller() { `name as other`(); plain(); `when`.api.plain() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.kt", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(
        calls
            .iter()
            .filter(|e| e.resolution == "resolved")
            .map(|e| e.count)
            .sum::<u64>(),
        3,
        "{:?}",
        rows.rows
    );
    assert!(calls.iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.name == "`target as name`" && t.file == "Api.kt")
    }));
    Ok(())
}

#[test]
fn kotlin_escaped_local_bindings_shadow_the_same_unquoted_function_name() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.kt",
        "fun target() {}\nfun caller() { val `target` = 1; target() }\n",
    )?;
    p.build(false)?;
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
    Ok(())
}

#[test]
fn kotlin_quoted_generic_arguments_do_not_become_part_of_the_receiver_type_name() -> TestResult {
    let p = Project::new()?;
    p.write("Main.kt", "class Box<T> { fun run() {} }\nclass `Some type` { fun run() {} }\nfun caller(value: Box<`Some type`>) { value.run() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.kt", "caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", rows.rows);
    assert_eq!(
        calls[0].target.as_ref().map(|t| t.qualified_name.as_str()),
        Some("Box::run"),
        "{:?}",
        rows.rows
    );
    Ok(())
}
