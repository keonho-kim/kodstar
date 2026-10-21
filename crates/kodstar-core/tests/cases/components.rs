// Purpose: 혼합 입력의 원본 바이트·행 위치 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn all_mixed_inputs_keep_embedded_use_locations_in_the_original_bytes() -> TestResult {
    let cases = [
        (
            "sample.vue",
            "<script lang=\"ts\">\nfunction target() {}\nfunction caller() { target(); }\n</script>\n<template><div>한글</div></template>",
            "target",
            "caller",
        ),
        (
            "sample.svelte",
            "<script>\nfunction target() {}\nfunction caller() { target(); }\n</script>\n<div>한글</div>",
            "target",
            "caller",
        ),
        (
            "sample.astro",
            "---\nfunction target() {}\nfunction caller() { target(); }\n---\n<div>한글</div>",
            "target",
            "caller",
        ),
        (
            "sample.ejs",
            "<% function target() {}\nfunction caller() { target(); } %>\n<div>한글</div>",
            "target",
            "caller",
        ),
        (
            "sample.blade.php",
            "@php\nfunction target() {}\nfunction caller() { target(); }\n@endphp\n<div>한글</div>",
            "target",
            "caller",
        ),
        (
            "sample.razor",
            "@code {\nvoid target() {}\nvoid caller() { target(); }\n}\n<div>한글</div>",
            "target",
            "caller",
        ),
    ];
    for (file, source, target, caller) in cases {
        let project = Project::new()?;
        project.write(file, source)?;
        project
            .build(false)
            .map_err(|error| format!("{file}: {error}"))?;
        let result = project.query(Query::Deps {
            selector: selector(file, caller),
        })?;
        let calls: Vec<_> = relationships(&result)
            .into_iter()
            .filter(|edge| edge.relation == "calls")
            .collect();
        assert_eq!(calls.len(), 1, "{file}: {calls:?}");
        assert_eq!(
            calls[0].target.as_ref().map(|symbol| symbol.name.as_str()),
            Some(target),
            "{file}"
        );
        let location = calls[0].site.span;
        assert_eq!(
            &source[location.start_byte..location.end_byte],
            target,
            "{file}"
        );
        let line = source[..location.start_byte]
            .bytes()
            .filter(|&byte| byte == b'\n')
            .count()
            + 1;
        assert_eq!(location.line, line, "{file}");
    }
    Ok(())
}
