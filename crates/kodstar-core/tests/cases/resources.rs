// Purpose: 조회·중첩 추출·panic의 예산과 자원 수명 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/src/infrastructure/index/mod.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;
use std::fs;

#[test]
fn cached_rows_exceeding_the_read_budget_return_a_limit_and_incomplete_summary() -> TestResult {
    let project = Project::new()?;
    project.write("large.rs", &format!("fn {}() {{}}", "a".repeat(1_800_000)))?;
    let build_config = project.directory.join("build.toml");
    fs::write(&build_config, "jobs = 1\nfact_batch_bytes = 8388608\n")?;
    let mut options = project.options();
    options.config = Some(build_config);
    project.engine.init(
        &options,
        InitOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    let read_config = project.directory.join("read.toml");
    fs::write(
        &read_config,
        // Cover history/snapshot caches while keeping the combined symbol row above the read limit.
        "jobs = 1\nmax_file_bytes = 1048576\nmemory_budget = 12582912\nfact_batch_bytes = 512\nqueue_bytes = 1024\ndb_cache_bytes = 1024\n",
    )?;
    options.config = Some(read_config);
    let mut sink = Collected::default();
    let error = project
        .engine
        .query(
            &options,
            &Query::Symbols {
                file: "large.rs".into(),
            },
            &QueryOptions {
                refresh: Refresh::None,
                ..QueryOptions::default()
            },
            &Cancellation::default(),
            &mut sink,
        )
        .expect_err("read budget must apply to facts already cached under a larger budget");
    assert_eq!(error.kind, ErrorKind::Limit);
    assert!(!sink.summary.ok_or("missing incomplete summary")?.complete);
    Ok(())
}

#[test]
fn nested_use_trees_keep_the_alias_and_original_use_location() -> TestResult {
    let project = Project::new()?;
    let mut source = String::from("pub use crate::{");
    for depth in 0..300 {
        source.push_str(&format!("branch_{depth}_{}::{{", "a".repeat(32)));
    }
    let offset = source.len();
    source.push_str("target as alias");
    source.push_str(&"}".repeat(301));
    source.push_str(";\nfn caller() { alias(); }\n");
    project.write("main.rs", &source)?;
    project.build(false)?;
    let sink = project.query(Query::Explain {
        selector: selector("main.rs", "main.rs"),
    })?;
    let imports = relationships(&sink);
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].relation, "reexports");
    assert_eq!(imports[0].site.span.start_byte, offset);
    assert_eq!(
        &source[imports[0].site.span.start_byte..imports[0].site.span.end_byte],
        "target as alias"
    );
    let call = project.query(Query::Explain {
        selector: selector("main.rs", "caller"),
    })?;
    assert_eq!(relationships(&call)[0].expression, "alias");
    Ok(())
}

#[test]
fn panicking_progress_rolls_back_and_joins_owned_workers() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn stable() {}")?;
    let first = project.build(false)?;
    project.write("main.rs", "fn changed() {}")?;
    project.write(
        "large.rs",
        &format!(
            "fn target() {{}} fn caller() {{ {} }}",
            "target();".repeat(5000)
        ),
    )?;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        project.engine.init(
            &project.options(),
            InitOptions::default(),
            &Cancellation::default(),
            &mut |_| -> Result<()> { panic!("consumer progress failure") },
        )
    }));
    assert!(outcome.is_err());
    let cached = project.query(Query::Symbols {
        file: "main.rs".into(),
    })?;
    assert_eq!(
        cached.metadata.ok_or("metadata")?.generation,
        Some(first.generation)
    );
    assert!(
        cached
            .rows
            .iter()
            .any(|row| matches!(row, QueryRow::Symbol(symbol) if symbol.name == "stable"))
    );
    project.engine.cache(
        &project.options(),
        CacheRequest::Clean { all: false },
        &Cancellation::default(),
        &mut Collected::default(),
    )?;
    assert!(!std::path::Path::new(&first.cache).exists());
    Ok(())
}
