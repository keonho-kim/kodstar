// Purpose: 조건 정규화·모순·희소 조합 저장과 실제 코드 영역 coverage 검증.
// Pattern: Unit Test.
// Usage: parser의 조건부 계획을 I/O 없이 검사.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/mod.rs.

use super::*;

#[test]
fn equivalent_conditions_share_atoms_without_comment_or_spacing_noise() -> Result<()> {
    assert_eq!(parse("defined (A) /* note */", 0)?, parse("defined(A)", 0)?);
    assert_eq!(parse("A > 2 // note", 0)?, parse("A>2", 0)?);
    assert_ne!(parse("definedFoo", 0)?, parse("defined(Foo)", 0)?);
    let condition = parse("defined(A) && !defined(A)", 0)?;
    assert!(condition.witness(&Cancellation::default())?.is_none());
    Ok(())
}

#[test]
fn shared_names_keep_many_independent_choices_within_a_small_budget() -> Result<()> {
    let source = (0..64)
        .map(|i| format!("#if OPTION_{i}\nvoid f{i}() {{}}\n#endif\n"))
        .collect::<String>();
    let cancel = Cancellation::default();
    let plan = Plan::new(Language::C, source.as_bytes(), 65536, &cancel)?.unwrap();
    assert_eq!(plan.variables.len(), 64);
    let mut coverage = plan.coverage(source.as_bytes(), &cancel)?;
    assert!(!coverage.complete());
    let mut choices = plan.projections(source.as_bytes(), &cancel);
    let mut count = 0;
    while let Some(projection) = choices.next_projection(&coverage, count != 0)? {
        coverage.record(&projection);
        count += 1;
    }
    assert_eq!(count, 2);
    assert!(coverage.complete());
    Ok(())
}

#[test]
fn long_else_if_chains_fail_with_a_limit_instead_of_growing_recursive_state() {
    let source = format!("#if FIRST\n{}#endif\n", "#elif NEXT\n".repeat(129));
    let result = Plan::new(
        Language::C,
        source.as_bytes(),
        1048576,
        &Cancellation::default(),
    );
    assert!(matches!(result, Err(error) if error.kind == ErrorKind::Limit));
}

#[test]
fn quoted_operators_and_parentheses_do_not_change_boolean_structure() -> Result<()> {
    assert!(matches!(
        parse("'(' == '(' || FEATURE", 0)?.as_ref(),
        Expr::Or(_, _)
    ));
    assert!(matches!(
        parse("VALUE == '||' && FEATURE", 0)?.as_ref(),
        Expr::And(_, _)
    ));
    assert_eq!(parse("('(' == '(')", 0)?, parse("'(' == '('", 0)?);
    Ok(())
}

#[test]
fn hundreds_of_independent_conditions_only_retain_two_covering_choices() -> Result<()> {
    let source = (0..400)
        .map(|i| format!("#if OPTION_{i}\nvoid f{i}() {{}}\n#else\nvoid g{i}() {{}}\n#endif\n"))
        .collect::<String>();
    let source = format!("#ifndef FEATURE_HEADER\n#define FEATURE_HEADER\n{source}#endif\n");
    let cancel = Cancellation::default();
    let plan = Plan::new(Language::C, source.as_bytes(), 1048576, &cancel)?.unwrap();
    let mut coverage = plan.coverage(source.as_bytes(), &cancel)?;
    let mut choices = plan.projections(source.as_bytes(), &cancel);
    let mut count = 0;
    while let Some(projection) = choices.next_projection(&coverage, count != 0)? {
        coverage.record(&projection);
        count += 1;
    }
    assert_eq!(count, 2);
    assert!(coverage.complete());
    Ok(())
}
