// Purpose: Clang의 동기 실행과 FFI 자원 수명을 안전한 Rust 호출로 제한.
// Pattern: Native Adapter.
// Usage: kodstar-core가 체크포인트 입력 provider와 예산을 전달.
// Related: crates/kodstar-clang/src/ffi.rs, crates/kodstar-clang/src/callbacks.rs, crates/kodstar-clang/src/model.rs.

mod callbacks;
mod facts;
mod ffi;
mod headers;
#[cfg(test)]
mod limits_tests;
mod model;
mod text;
pub use facts::Facts;
pub use headers::HeaderDecoder;
pub use model::{
    Analysis, Fact, Input, InputError, Inputs, Issue, IssueKind, Limits, Source, Status,
};
use std::ffi::CString;

/// Zstd-compressed original header license notices, decoded by the CLI on demand.
pub const HEADER_NOTICES_ZSTD: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/header-notices.zst"));

pub const VERSION: &str = "23.1.4";
pub const RESOURCE_ID: &str = env!("KODSTAR_CLANG_RESOURCE_ID");
pub const ENGINE_ID: &str = env!("KODSTAR_CLANG_ENGINE_ID");

pub const TARGET: &str = env!("KODSTAR_CLANG_TARGET");

/// Arguments must already be restricted to analysis flags; this API never runs a compiler subprocess.
pub fn analyze(input: Input<'_>, provider: &mut dyn Inputs, headers: &HeaderDecoder) -> Analysis {
    if input.limits.memory_bytes == 0 {
        return Analysis {
            status: Status::Limited,
            facts: Facts::default(),
            issue: Some(Issue {
                kind: IssueKind::NativeMemory,
                subject: String::new(),
                used: 1,
                limit: 0,
            }),
        };
    }
    if input.source.len() > input.limits.input_bytes {
        return Analysis {
            status: Status::Limited,
            facts: Facts::default(),
            issue: Some(Issue {
                kind: IssueKind::InputBytes,
                subject: String::new(),
                used: input.source.len(),
                limit: input.limits.input_bytes,
            }),
        };
    }
    let invalid = || Analysis {
        status: Status::InvalidInput,
        facts: Facts::default(),
        issue: None,
    };
    let Ok(path) = CString::new(input.path) else {
        return invalid();
    };
    let (target, target_size) = input.target.unwrap_or((input.path, input.source.len()));
    if !target.starts_with("/__kodstar_project/") || target.as_bytes().contains(&0) {
        return invalid();
    }
    if !input.path.starts_with("/__kodstar_project/")
        || simdutf8::basic::from_utf8(input.source).is_err()
    {
        return invalid();
    }
    let arguments: Result<Vec<_>, _> = input
        .arguments
        .iter()
        .map(|s| CString::new(s.as_str()))
        .collect();
    let Ok(arguments) = arguments else {
        return invalid();
    };
    let pointers: Vec<_> = arguments.iter().map(|s| s.as_ptr()).collect();
    let mut state = callbacks::State::new(
        provider,
        input.limits,
        input.source.len(),
        target_size,
        headers,
    );
    let callbacks = state.callbacks();
    let raw = ffi::Input {
        path: ffi::Text {
            data: path.as_ptr(),
            size: input.path.len(),
        },
        source: ffi::Text {
            data: input.source.as_ptr().cast(),
            size: input.source.len(),
        },
        target: ffi::Text {
            data: target.as_ptr().cast(),
            size: target.len(),
        },
        arguments: pointers.as_ptr(),
        argument_count: pointers.len(),
        token_limit: input.limits.tokens,
        memory_limit: input.limits.memory_bytes,
        template_limit: input.limits.templates,
        path_limit: input.limits.paths,
        cpp: i32::from(input.cpp),
    };
    // Views/state outlive the synchronous call; all native callback buffers are released before return.
    let result = unsafe { ffi::ks_clang_analyze(&raw, &callbacks) };
    let status = state.status.unwrap_or(match result {
        0 => Status::Complete,
        2 => Status::Limited,
        _ => Status::Incomplete,
    });
    Analysis {
        status,
        issue: state.issue,
        facts: if state.reusable_declarations
            && matches!(status, Status::Complete | Status::Incomplete)
        {
            state.facts
        } else {
            Facts::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Closed {
        checks: Cell<usize>,
        stop: usize,
    }
    impl Inputs for Closed {
        fn read(&mut self, _: &str, _: bool) -> Result<Source, InputError> {
            Err(InputError::Missing)
        }
        fn cancelled(&self) -> bool {
            let next = self.checks.get() + 1;
            self.checks.set(next);
            next >= self.stop
        }
    }
    #[test]
    fn builtin_headers_and_semantic_facts_use_only_the_memory_filesystem() {
        let mut input = Closed {
            checks: Cell::new(0),
            stop: usize::MAX,
        };
        let source=b"#include <stddef.h>\nint pick(int n){return n;} double pick(double n){return n;} int caller(){return pick(1);}";
        let result = analyze(
            Input {
                target: None,
                path: "/__kodstar_project/main.cpp",
                source,
                cpp: true,
                arguments: &[],
                limits: Limits::for_workspace(32 * 1024 * 1024),
            },
            &mut input,
            &HeaderDecoder::default(),
        );
        assert_eq!(result.status, Status::Complete);
        assert!(
            result
                .facts
                .iter()
                .any(|f| f.kind == 2 && f.name == "pick" && f.usr.contains("#I"))
        );
    }
    #[test]
    fn cancellation_and_tiny_output_budgets_never_publish_partial_native_facts() {
        for (stop, budget, status) in [
            (1, 4 * 1024 * 1024, Status::Cancelled),
            (usize::MAX, 1, Status::Limited),
        ] {
            let mut input = Closed {
                checks: Cell::new(0),
                stop,
            };
            let result = analyze(
                Input {
                    target: None,
                    path: "/__kodstar_project/main.c",
                    source: b"int target(void){return 1;}",
                    cpp: false,
                    arguments: &[],
                    limits: Limits::for_workspace(budget),
                },
                &mut input,
                &HeaderDecoder::default(),
            );
            assert_eq!(result.status, status);
            assert!(result.facts.is_empty());
        }
    }

    #[test]
    fn template_instantiation_has_an_independent_work_budget() {
        let mut provider = Closed {
            checks: Cell::new(0),
            stop: usize::MAX,
        };
        let source = b"template<int N,int I>struct B{B<N-1,I*2> left;B<N-1,I*2+1> right;};template<int I>struct B<0,I>{}; B<12,0> value;";
        let result = analyze(
            Input {
                target: None,
                path: "/__kodstar_project/main.cpp",
                source,
                cpp: true,
                arguments: &[],
                limits: Limits::for_workspace(32768),
            },
            &mut provider,
            &HeaderDecoder::default(),
        );
        assert_eq!(result.status, Status::Limited);
        assert!(result.facts.is_empty());
    }

    #[test]
    fn suppressed_time_warnings_cannot_make_clock_dependent_facts_reusable() {
        let mut provider = Closed {
            checks: Cell::new(0),
            stop: usize::MAX,
        };
        let result = analyze(
            Input {
                target: None,
                path: "/__kodstar_project/main.c",
                source: b"#pragma clang diagnostic ignored \"-Wdate-time\"\nint caller(void){return __DATE__[0];}",
                cpp: false,
                arguments: &[],
                limits: Limits::for_workspace(32 * 1024 * 1024),
            },
            &mut provider,
            &HeaderDecoder::default(),
        );
        assert_eq!(result.status, Status::Incomplete);
        assert!(result.facts.is_empty());
    }

    #[test]
    fn cancellation_during_preprocessing_never_publishes_native_results() {
        let mut provider = Closed {
            checks: Cell::new(0),
            stop: 30,
        };
        let source = format!(
            "int values[]={{ {} }};int caller(){{return 1;}}",
            "1,".repeat(100000)
        );
        let result = analyze(
            Input {
                target: None,
                path: "/__kodstar_project/main.cpp",
                source: source.as_bytes(),
                cpp: true,
                arguments: &[],
                limits: Limits::for_workspace(64 * 1024 * 1024),
            },
            &mut provider,
            &HeaderDecoder::default(),
        );
        assert_eq!(result.status, Status::Cancelled);
        assert!(result.facts.is_empty());
        assert!(provider.checks.get() >= 30);
    }
}
