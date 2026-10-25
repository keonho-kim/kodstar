// Purpose: 공유 문자열·바이너리 정체성의 회수·rollback·손상 검출 검증.
// Pattern: Storage Regression Test.
// Usage: engine 회귀가 공개 결과와 실제 저장 수명을 함께 대조.
// Related: crates/kodstar-core/src/infrastructure/index/atoms/mod.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;

fn database(
    report: &InitReport,
) -> std::result::Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let c = rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    postings::register(&c)?;
    Ok(c)
}

#[test]
fn shared_values_and_binary_keys_survive_incremental_collection() -> TestResult {
    let p = Project::new()?;
    p.write("go.mod", "module example.test/atoms\ngo 1.23\n")?;
    for name in ["FirstAtom", "SecondAtom", "ThirdAtom"] {
        p.write(
            "api.go",
            &format!("package atoms\nfunc {name}(){{}}\nfunc Caller(){{{name}();{name}()}}\n"),
        )?;
        let report = p.build(false)?;
        let c = database(&report)?;
        let unique: u64 =
            c.query_row("SELECT count(*) FROM atoms WHERE value=?1", [name], |r| {
                r.get(0)
            })?;
        assert_eq!(unique, 1);
        let ids:u64=c.query_row("SELECT count(DISTINCT a.id) FROM atoms a JOIN facts_rows f ON f.expression=a.id WHERE a.value=?1",[name],|r|r.get(0))?;
        assert_eq!(ids, 1, "all uses must share one expression value");
        let invalid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM symbols_rows WHERE length(syntax_key)<>32) OR EXISTS(SELECT 1 FROM atoms WHERE refs=0)",[],|r|r.get(0))?;
        assert!(!invalid);
        if name != "FirstAtom" {
            assert_eq!(
                c.query_row(
                    "SELECT count(*) FROM atoms WHERE value='FirstAtom'",
                    [],
                    |r| r.get::<_, u64>(0)
                )?,
                0
            );
        }
        drop(c);
        let partial = normalized_index(&report)?;
        assert_eq!(partial, normalized_index(&p.build(true)?)?);
    }
    Ok(())
}

#[test]
fn record_changes_are_atomic_and_referenced_atoms_cannot_be_removed() -> TestResult {
    let p = Project::new()?;
    p.write("api.py", "def target(): pass\ndef caller(): target()\n")?;
    let report = p.build(false)?;
    let expected = normalized_index(&report)?;
    let c = database(&report)?;
    c.execute_batch("BEGIN; UPDATE facts SET expression='temporary_atom_for_rollback' WHERE id=(SELECT min(id) FROM facts);")?;
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM atoms WHERE value='temporary_atom_for_rollback'",
            [],
            |r| r.get::<_, u64>(0)
        )?,
        1
    );
    c.execute_batch("ROLLBACK;")?;
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM atoms WHERE value='temporary_atom_for_rollback'",
            [],
            |r| r.get::<_, u64>(0)
        )?,
        0
    );
    assert!(
        c.execute(
            "DELETE FROM atoms WHERE id=(SELECT id FROM atoms WHERE refs>0 LIMIT 1)",
            []
        )
        .is_err()
    );
    assert!(c.execute("UPDATE facts_rows SET expression=9223372036854775807 WHERE id=(SELECT min(id) FROM facts_rows)",[]).is_err());
    let error=c.execute("INSERT INTO facts_rows(file_id,source_id,scope_id,expression,relation,language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte) SELECT file_id,source_id,scope_id,9223372036854775807,relation,language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte FROM facts_rows LIMIT 1",[]).expect_err("missing atom must abort insert");
    assert!(
        error.to_string().contains("missing analysis string atom"),
        "{error}"
    );
    assert!(
        c.execute(
            "UPDATE atoms SET value='replacement' WHERE id=(SELECT min(id) FROM atoms)",
            []
        )
        .is_err()
    );
    drop(c);
    assert_eq!(expected, normalized_index(&report)?);
    Ok(())
}

#[test]
fn doctor_detects_inaccurate_shared_value_counts() -> TestResult {
    let p = Project::new()?;
    p.write("api.py", "def target(): pass\n")?;
    let report = p.build(false)?;
    let c = database(&report)?;
    c.execute(
        "UPDATE atoms SET refs=refs+1 WHERE id=(SELECT min(id) FROM atoms)",
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
        .expect_err("incorrect reference count");
    assert!(
        error.to_string().contains("string reference counts"),
        "{error:?}"
    );
    Ok(())
}
