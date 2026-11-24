// Purpose: 독립 native 제한·부분 결과 거절·진단 원문 비노출 회귀.
// Pattern: Unit Test.
// Usage: kodstar-clang test가 닫힌 입력 provider로 각 실패 경계를 검증.
// Related: crates/kodstar-clang/src/model.rs, crates/kodstar-clang/src/lib.rs.

use super::*;

#[test]
fn repeated_native_names_share_the_file_owned_text_storage() {
    let result = analyze(
        Input {
            target: None,
            path: "/__kodstar_project/main.c",
            source: b"int leaf(void){return 1;} int caller(void){return leaf()+leaf();}",
            cpp: false,
            arguments: &[],
            limits: Limits::for_workspace(32 * 1024 * 1024),
        },
        &mut Closed,
        &HeaderDecoder::default(),
    );
    assert_eq!(result.status, Status::Complete);
    let calls: Vec<_> = result.facts.iter().filter(|fact| fact.kind == 2).collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].usr, calls[1].usr);
    assert_eq!(calls[0].usr.as_ptr(), calls[1].usr.as_ptr());
}

#[test]
fn native_work_limits_are_independent_of_memory_and_reset_per_invocation() {
    let small = Limits::for_workspace(32 * 1024 * 1024);
    let large = Limits::for_workspace(128 * 1024 * 1024);
    assert_eq!(
        (small.tokens, small.templates, small.paths),
        (large.tokens, large.templates, large.paths)
    );
    let mut limits = large;
    limits.tokens = 100;
    let source = format!(
        "int values[]={{ {} }};int leaf(){{return 1;}}",
        "1,".repeat(30)
    );
    for _ in 0..3 {
        let result = analyze(
            Input {
                target: None,
                path: "/__kodstar_project/main.cpp",
                source: source.as_bytes(),
                cpp: true,
                arguments: &[],
                limits,
            },
            &mut Closed,
            &HeaderDecoder::default(),
        );
        assert_eq!(result.status, Status::Complete, "{:?}", result.issue);
    }
}

#[test]
fn native_memory_reports_measured_usage_and_does_not_publish_partial_facts() {
    let mut limits = Limits::for_workspace(32 * 1024 * 1024);
    limits.memory_bytes = 1;
    let result = analyze(
        Input {
            target: None,
            path: "/__kodstar_project/main.c",
            source: b"int leaf(void){return 1;}",
            cpp: false,
            arguments: &[],
            limits,
        },
        &mut Closed,
        &HeaderDecoder::default(),
    );
    assert_eq!(result.status, Status::Limited);
    assert!(result.facts.is_empty());
    let issue = result.issue.expect("memory diagnostic");
    assert_eq!(issue.kind, IssueKind::NativeMemory);
    assert!(issue.used > issue.limit);
}

struct Closed;
impl Inputs for Closed {
    fn read(&mut self, _: &str, _: bool) -> Result<Source, InputError> {
        Err(InputError::Missing)
    }
    fn cancelled(&self) -> bool {
        false
    }
}

#[test]
fn independent_limits_explain_their_failure_and_discard_partial_facts() {
    for kind in [
        IssueKind::InputBytes,
        IssueKind::FactBytes,
        IssueKind::Tokens,
        IssueKind::Templates,
    ] {
        let mut limits = Limits::for_workspace(32 * 1024 * 1024);
        let source = match kind {
            IssueKind::InputBytes => {
                limits.input_bytes = 1;
                b"int leaf(){return 1;}".as_slice()
            }
            IssueKind::FactBytes => {
                limits.fact_bytes = 1;
                b"int leaf(){return 1;}".as_slice()
            }
            IssueKind::Tokens => {
                limits.tokens = 1;
                b"int leaf(){return 1;}".as_slice()
            }
            IssueKind::Templates => {
                limits.templates = 1;
                b"template<int N>struct A{A<N-1> inner;};template<>struct A<0>{};A<8> value;"
                    .as_slice()
            }
            _ => unreachable!(),
        };
        let result = analyze(
            Input {
                target: None,
                path: "/__kodstar_project/main.cpp",
                source,
                cpp: true,
                arguments: &[],
                limits,
            },
            &mut Closed,
            &HeaderDecoder::default(),
        );
        assert_eq!(result.status, Status::Limited, "{kind:?}");
        assert!(result.facts.is_empty());
        let issue = result.issue.expect("structured limit reason");
        assert_eq!(issue.kind, kind);
        assert!(issue.used > issue.limit, "{issue:?}");
    }
}

#[test]
fn compiler_diagnostics_do_not_persist_user_error_text() {
    let result = analyze(
        Input {
            target: None,
            path: "/__kodstar_project/main.c",
            source: b"#error PRIVATE_SOURCE_TEXT\n",
            cpp: false,
            arguments: &[],
            limits: Limits::for_workspace(32 * 1024 * 1024),
        },
        &mut Closed,
        &HeaderDecoder::default(),
    );
    assert_eq!(result.status, Status::Incomplete);
    let issue = result.issue.expect("compiler diagnostic");
    assert_eq!(issue.kind, IssueKind::CompilerError);
    assert!(!issue.subject.contains("PRIVATE_SOURCE_TEXT"));
    assert!(issue.subject.contains("main.c:1"));
}

#[test]
#[cfg(target_os = "macos")]
fn directory_prefix_search_skips_a_same_named_header_sibling() {
    assert!(matches!(
        headers::read(
            &HeaderDecoder::default(),
            "/__kodstar_clang/system/sys/_types",
            false
        ),
        Ok(Source::Directory)
    ));
    assert!(matches!(
        headers::read(
            &HeaderDecoder::default(),
            "/__kodstar_clang/system/sys/_types/_int8_t.h",
            false
        ),
        Ok(Source::File { .. })
    ));
}
