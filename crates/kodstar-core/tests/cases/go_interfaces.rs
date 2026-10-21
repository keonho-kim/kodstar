// Purpose: Go interface method set·embedding·구현 후보와 무효화 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn go_method_sets_keep_contract_calls_and_possible_implementations_separate() -> TestResult {
    let project = Project::new()?;
    project.write("api.go","package api\ntype Reader interface { Read(input []byte) (int,error) }\ntype Worker struct {}\nfunc (w Worker) Read(other []byte) (int,error) { return 0,nil }\ntype Wrong struct {}\nfunc (w Wrong) Read() (int,error) { return 0,nil }\nfunc caller(value Reader) { value.Read(nil) }\n")?;
    project.build(false)?;
    let rows = project.query(Query::Explain {
        selector: selector("api.go", "caller"),
    })?;
    assert!(relationships(&rows).iter().any(|e| {
        e.relation == "calls"
            && e.resolution == "resolved"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Reader::Read")
    }));
    assert!(relationships(&rows).iter().any(|e| {
        e.relation == "dispatch_candidate"
            && e.resolution == "candidate"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Worker::Read")
    }));
    assert!(!relationships(&rows).iter().any(|e| {
        e.target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Wrong::Read")
    }));
    Ok(())
}

#[test]
fn go_embedded_fields_promote_members_and_interface_requirements() -> TestResult {
    let project = Project::new()?;
    project.write("api.go","package api\ntype Base interface { Run() }\ntype Full interface { Base; Close() }\ntype Worker struct {}\nfunc (w Worker) Run() {}\nfunc (w Worker) Close() {}\ntype Service struct { Worker }\nfunc caller(value Service) { value.Run(); value.Worker.Close() }\nfunc contracted(value Full) { value.Run() }\n")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("api.go", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls")
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    let contract = project.query(Query::Explain {
        selector: selector("api.go", "contracted"),
    })?;
    assert!(relationships(&contract).iter().any(|e| {
        e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Base::Run")
    }));
    Ok(())
}

#[test]
fn go_interface_candidates_are_invalidated_when_method_sets_change() -> TestResult {
    let project = Project::new()?;
    project.write("contract.go", "package api\ntype Reader interface { Read(a,b int) int }\nfunc caller(value Reader) { value.Read(1,2) }")?;
    let correct =
        "package api\ntype Worker struct {}\nfunc (w *Worker) Read(x,y int) int { return x+y }";
    project.write("worker.go", correct)?;
    project.build(false)?;
    let query = || Query::Explain {
        selector: selector("contract.go", "caller"),
    };
    let targets = |rows: &Collected| -> Vec<String> {
        relationships(rows)
            .iter()
            .filter(|e| e.relation == "dispatch_candidate")
            .filter_map(|e| e.target.as_ref().map(|t| t.logical_key.clone()))
            .collect()
    };
    assert_eq!(targets(&project.query(query())?).len(), 1);
    project.write(
        "worker.go",
        "package api\ntype Worker struct {}\nfunc (w *Worker) Read(x int) int { return x }",
    )?;
    assert_eq!(project.build(false)?.parsed_files, 1);
    assert!(targets(&project.query(query())?).is_empty());
    project.write("worker.go", correct)?;
    project.build(false)?;
    let incremental = targets(&project.query(query())?);
    project.build(true)?;
    assert_eq!(incremental, targets(&project.query(query())?));
    Ok(())
}
