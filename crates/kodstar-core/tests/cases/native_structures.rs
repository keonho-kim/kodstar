// Purpose: native 선언 근거·헤더 소유 문맥과 enum 매크로의 원문 위치 검증.
// Pattern: Integration Test.
// Usage: engine 테스트에서 외부 compiler·프로젝트 빌드 없이 실행.
// Related: crates/kodstar-core/src/infrastructure/clang/structural.rs, crates/kodstar-core/src/infrastructure/clang/configuration/context.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;

#[test]
fn enum_maps_and_prototype_wrappers_keep_real_definitions() -> TestResult {
    for (file, source, names) in [
        (
            "main.c",
            "#define VALUES(X) X(READY) X(DONE)\n#define ENUM(N) N,\nenum State { VALUES(ENUM) LAST };\n#define PROTO(args) args\nint target PROTO((int value));\nint caller(void) { return target(READY); }\n",
            vec!["READY", "DONE", "target", "caller"],
        ),
        (
            "main.cpp",
            "#define VALUES(X) X(READY) X(DONE)\n#define ENUM(N) N,\nenum class State { VALUES(ENUM) LAST };\nint caller() { return static_cast<int>(State::READY); }\n",
            vec!["READY", "DONE", "caller"],
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        let report = p.build(false)?;
        assert_eq!(report.coverage.failed_files, 0, "{file}");
        let rows = p.query(Query::Symbols { file: file.into() })?;
        for name in names {
            assert!(
                rows.rows
                    .iter()
                    .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == name)),
                "missing {name} in {file}"
            );
        }
        let actual = normalized_index(&report)?;
        assert_eq!(actual, normalized_index(&p.build(true)?)?);
    }
    Ok(())
}

#[test]
fn inline_header_uses_a_proven_owner_and_invalidates_when_that_owner_changes() -> TestResult {
    let p = Project::new()?;
    p.write(
        "include/api.h",
        "#pragma once\nstatic inline int target(void) { return 1; }\n#include \"api-inl.h\"\n",
    )?;
    let inline = format!(
        "/*{}*/\nstatic inline int caller(void) {{ return target(); }}\n",
        "padding".repeat(100)
    );
    p.write("include/api-inl.h", &inline)?;
    let first = p.build(false)?;
    assert_eq!(first.coverage.failed_files, 0);
    assert_eq!(first.coverage.native_incomplete_files, 0);
    let rows = p.query(Query::Deps {
        selector: selector("include/api-inl.h", "caller"),
    })?;
    let edges = relationships(&rows);
    let call = edges
        .iter()
        .find(|r| r.target.as_ref().is_some_and(|s| s.name == "target"))
        .ok_or("owned header call")?;
    assert_eq!(
        &inline.as_bytes()[call.site.span.start_byte..call.site.span.end_byte],
        b"target"
    );
    p.write(
        "include/api.h",
        "#pragma once\nstatic inline int target(void) { return 1; }\n",
    )?;
    let changed = p.build(false)?;
    assert!(changed.coverage.native_incomplete_files > 0);
    let actual = normalized_index(&changed)?;
    assert_eq!(actual, normalized_index(&p.build(true)?)?);
    Ok(())
}
