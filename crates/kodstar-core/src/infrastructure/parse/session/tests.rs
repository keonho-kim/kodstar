// Purpose: 파일 내 AST 재사용과 복수 AST·취소의 수명 경계 검증.
// Pattern: Unit Test.
// Usage: parser session이 같은 트리의 실제 소유권 이전을 검사.
// Related: crates/kodstar-core/src/infrastructure/parse/session.rs.

use super::*;

#[test]
fn a_single_ast_is_transferred_to_the_second_walk_and_cleared_between_files() -> Result<()> {
    let grammar = tree_sitter_rust::LANGUAGE.into();
    let cancel = Cancellation::default();
    let mut parser = ParseSession::new(65536);
    parser.begin_file(true);
    let first = parser.parse_tree(&grammar, Path::new("a.rs"), b"fn first() {}", &cancel)?;
    parser.replay();
    let second = parser.parse_tree(&grammar, Path::new("a.rs"), b"fn first() {}", &cancel)?;
    // Tree::clone copies its root wrapper; immutable child subtrees retain their allocation.
    let first_id = first.root_node().named_child(0).expect("function").id();
    assert_eq!(
        first_id,
        second.root_node().named_child(0).expect("function").id()
    );
    assert!(parser.tree.is_none());
    parser.end_file();
    parser.begin_file(true);
    let next = parser.parse_tree(&grammar, Path::new("b.rs"), b"fn first() {}", &cancel)?;
    assert_ne!(
        first_id,
        next.root_node().named_child(0).expect("function").id()
    );
    parser.end_file();
    assert!(parser.tree.is_none());
    Ok(())
}

#[test]
fn multiple_asts_drop_retention_and_replay_still_checks_cancellation() -> Result<()> {
    let grammar = tree_sitter_rust::LANGUAGE.into();
    let cancel = Cancellation::default();
    let mut parser = ParseSession::new(65536);
    parser.begin_file(true);
    parser.parse_tree(&grammar, Path::new("a.rs"), b"fn first() {}", &cancel)?;
    assert!(parser.tree.is_some());
    parser.parse_tree(&grammar, Path::new("a.rs"), b"fn second() {}", &cancel)?;
    assert!(parser.tree.is_none());
    parser.begin_file(true);
    parser.parse_tree(&grammar, Path::new("a.rs"), b"fn first() {}", &cancel)?;
    parser.replay();
    cancel.cancel();
    assert!(
        parser
            .parse_tree(&grammar, Path::new("a.rs"), b"fn first() {}", &cancel)
            .is_err()
    );
    parser.end_file();
    assert!(parser.tree.is_none());
    Ok(())
}
