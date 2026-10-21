// Purpose: 명시적 필드 타입과 중첩 receiver의 대상 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn changed_field_type_reresolves_an_unchanged_imported_impl() -> TestResult {
    let project = Project::new()?;
    project.write(
        "src/lib.rs",
        "mod types; use crate::types::*; impl Service { fn caller(&self) { self.worker.run(); } }",
    )?;
    let types = |field: &str| {
        format!(
            "pub struct Worker; impl Worker {{ fn run(&self) {{}} }} pub struct Other; impl Other {{ fn run(&self) {{}} }} pub struct Service {{ worker: {field} }}"
        )
    };
    project.write("src/types.rs", &types("Worker"))?;
    project.build(false)?;
    let query = || Query::Deps {
        selector: selector("src/lib.rs", "Service::caller"),
    };
    let initial = project.query(query())?;
    assert_eq!(
        relationships(&initial)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .qualified_name,
        "Worker::run"
    );
    project.write("src/types.rs", &types("Other"))?;
    let report = project.build(false)?;
    assert_eq!(report.parsed_files, 1);
    assert_eq!(report.coverage.reused_files, 1);
    let incremental = project.query(query())?;
    assert_eq!(
        relationships(&incremental)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .qualified_name,
        "Other::run"
    );
    project.build(true)?;
    let full = project.query(query())?;
    assert_eq!(
        relationships(&incremental)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .logical_key,
        relationships(&full)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .logical_key
    );
    Ok(())
}

#[test]
fn typed_fields_and_promoted_properties_support_nested_receivers() -> TestResult {
    for (file, source, caller, target) in [
        (
            "Main.java",
            "class Worker { void run() {} } class Service { Worker worker; void caller() { this.worker.run(); } }",
            "Service::caller",
            "Worker::run",
        ),
        (
            "main.cs",
            "class Worker { public void Run() {} } class Service { public Worker worker; void caller() { this.worker.Run(); } }",
            "Service::caller",
            "Worker::Run",
        ),
        (
            "main.cpp",
            "namespace api { struct Worker { void run() {} }; struct Service { Worker worker; void caller() { this->worker.run(); } }; }",
            "api::Service::caller",
            "api::Worker::run",
        ),
        (
            "main.php",
            "<?php class Worker { public function run() {} } class Service { public function __construct(public Worker $worker) {} public function caller() { $this->worker->run(); } }",
            "Service::caller",
            "Worker::run",
        ),
        (
            "lib.rs",
            "struct Worker; impl Worker { fn run(&self) {} } struct Service { worker: Worker } impl Service { fn caller(&self) { self.worker.run(); } }",
            "Service::caller",
            "Worker::run",
        ),
        (
            "main.go",
            "package api\ntype Worker struct{}\nfunc (w Worker) Run() {}\ntype Service struct { worker Worker }\nfunc (s Service) caller() { s.worker.Run() }\n",
            "Service::caller",
            "Worker::Run",
        ),
    ] {
        let project = Project::new()?;
        project.write(file, source)?;
        project.build(false)?;
        let rows = project.query(Query::Deps {
            selector: selector(file, caller),
        })?;
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == target)),
            "{file}: {:?}",
            rows.rows
        );
    }
    Ok(())
}
