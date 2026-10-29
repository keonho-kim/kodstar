// Purpose: 형식 5 분기·삭제·원문과 형식 6 이전 상태의 전환·손상 검출 검증.
// Pattern: Storage Regression Test.
// Usage: 실제 이력 저장소를 구형 schema로 구성하고 명시적 전환을 실행.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/metadata/mod.rs, crates/kodstar-core/src/infrastructure/history/metadata.sql.

use crate::support::*;
use kodstar_core::*;
use rusqlite::Connection;

type Entry = (i64, String, Option<i64>, u32, String);
fn entries(c: &Connection) -> rusqlite::Result<Vec<Entry>> {
    c.prepare("SELECT checkpoint,hex(path),blob,mode,kind FROM entries ORDER BY checkpoint,path")?
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect()
}

#[test]
fn v5_branched_history_preserves_entries_and_every_restored_version() -> TestResult {
    let p = Project::new()?;
    p.write("a.py", "def first(): pass\n")?;
    p.write("deleted.txt", "preserved text\n")?;
    p.build(false)?;
    p.history(HistoryRequest::CreateBranch {
        name: "topic".into(),
    })?;
    p.write("a.py", "def second(): pass\n")?;
    std::fs::remove_file(p.root.join("deleted.txt"))?;
    p.build(false)?;
    p.history(HistoryRequest::SwitchBranch {
        name: "topic".into(),
    })?;
    p.write("a.py", "def branch(): pass\n")?;
    p.build(false)?;
    let c = Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    let expected = entries(&c)?;
    c.execute_batch(include_str!(
        "../../src/infrastructure/history/maintenance/fixtures/restore_v5.sql"
    ))?;
    c.execute_batch("PRAGMA user_version=5;")?;
    assert!(p.build(false).is_err());
    p.history(HistoryRequest::Upgrade)?;
    assert_eq!(entries(&c)?, expected);
    assert!(!c.query_row(
        "SELECT EXISTS(SELECT 1 FROM blobs WHERE typeof(hash)<>'blob' OR length(hash)<>32)",
        [],
        |r| r.get::<_, bool>(0)
    )?);
    assert!(c.query_row(
        "SELECT (SELECT count(*) FROM paths)<(SELECT count(*) FROM entry_rows)",
        [],
        |r| r.get::<_, bool>(0)
    )?);
    for (revision, expected, present) in [
        ("c1", "first", true),
        ("c2", "second", false),
        ("c3", "branch", true),
    ] {
        p.history(HistoryRequest::Restore {
            revision: revision.into(),
            path: None,
        })?;
        assert_eq!(
            std::fs::read_to_string(p.root.join("a.py"))?,
            format!("def {expected}(): pass\n")
        );
        assert_eq!(p.root.join("deleted.txt").exists(), present);
    }
    p.engine.status(
        &p.options(),
        true,
        &Cancellation::default(),
        &mut Collected::default(),
    )?;
    Ok(())
}

#[test]
fn doctor_rejects_a_previous_state_that_disagrees_with_its_parent() -> TestResult {
    let p = Project::new()?;
    p.write("a.py", "def first(): pass\n")?;
    p.build(false)?;
    p.write("a.py", "def other(): pass\n")?;
    p.build(false)?;
    let c = Connection::open(p.root.join(".kodstar/repository.sqlite"))?;
    c.execute(
        "UPDATE entry_rows SET before_blob=blob WHERE checkpoint=2",
        [],
    )?;
    drop(c);
    let error = p
        .engine
        .status(
            &p.options(),
            true,
            &Cancellation::default(),
            &mut Collected::default(),
        )
        .expect_err("inconsistent previous state accepted");
    assert!(error.to_string().contains("previous file state"), "{error}");
    Ok(())
}
