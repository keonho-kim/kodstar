// Purpose: 입력 오류와 저장 메타데이터에서 불필요한 원문 제외 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn configuration_errors_preserve_the_cause_without_echoing_values() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn target() {}")?;
    let path = project.directory.join("invalid.toml");
    for source in [
        "jobs = \"fixture-config-secret\"",
        "jobs = \"fixture-config-secret\" trailing",
    ] {
        std::fs::write(&path, source)?;
        let mut options = project.options();
        options.config = Some(path.clone());
        let error = project
            .engine
            .init(
                &options,
                InitOptions::default(),
                &Cancellation::default(),
                &mut |_| Ok(()),
            )
            .expect_err("invalid configuration");
        assert_eq!(error.kind, ErrorKind::Argument);
        assert!(!format!("{error} {error:?}").contains("fixture-config-secret"));
        assert!(std::error::Error::source(&error).is_some());
    }
    Ok(())
}

#[test]
fn cached_metadata_omits_call_arguments_type_bodies_and_attribute_payloads() -> TestResult {
    let project = Project::new()?;
    project.write("main.ts", "function factory(x: string) { return {}; } function caller() { factory(\"fixture-call-secret\").target(); }")?;
    project.write("main.go", "package sample\ntype Credentials struct { Token string `json:\"fixture-type-secret\"` }\nfunc accept(x struct { Token string `json:\"fixture-parameter-secret\"` }) {}\n")?;
    project.write(
        "lib.rs",
        "#[cfg_attr(feature = \"opt\", doc = \"fixture-doc-secret\")]\nfn target() {}",
    )?;
    project.write(
        "types.cpp",
        "void accept(int * /* fixture-declarator-secret */ input) {}",
    )?;
    let report = project.build(false)?;
    let secrets = [
        "fixture-call-secret",
        "fixture-type-secret",
        "fixture-parameter-secret",
        "fixture-doc-secret",
        "fixture-declarator-secret",
    ];
    for file in ["main.ts", "main.go", "lib.rs"] {
        let output =
            serde_json::to_string(&project.query(Query::Symbols { file: file.into() })?.rows)?;
        for secret in secrets {
            assert!(!output.contains(secret), "symbol output retained {secret}");
        }
    }
    let rows = project.query(Query::Explain {
        selector: selector("main.ts", "caller"),
    })?;
    let output = serde_json::to_string(&rows.rows)?;
    assert!(!output.contains(secrets[0]));
    assert!(
        relationships(&rows)
            .iter()
            .any(|edge| edge.resolution == "unresolved")
    );
    for entry in std::fs::read_dir(&report.cache)? {
        let path = entry?.path();
        if path.is_file() {
            let bytes = std::fs::read(path)?;
            for secret in secrets {
                assert!(
                    !bytes
                        .windows(secret.len())
                        .any(|window| window == secret.as_bytes()),
                    "cache retained {secret}"
                );
            }
        }
    }
    Ok(())
}
