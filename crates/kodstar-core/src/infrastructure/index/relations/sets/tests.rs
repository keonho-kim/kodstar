// Purpose: 공유 후보 정보의 복수 출처·개별 관계 ID·수정·rollback·마지막 참조 회수 검증.
// Pattern: Storage Regression Test.
// Usage: core 단위 검사가 실제 SQLite 관계 adapter를 사용.
// Related: crates/kodstar-core/src/infrastructure/index/relations/sets/mod.rs, crates/kodstar-core/src/infrastructure/index/relations/verification.rs.

use super::super::{Relation, SCHEMA, Writer, tests::parents};
use crate::domain::relations::Resolution;
use rusqlite::{Connection, Result};

#[test]
fn sharing_keeps_each_use_and_id_and_collects_only_after_the_last_reference() -> Result<()> {
    let c = Connection::open_in_memory()?;
    parents(&c)?;
    crate::infrastructure::index::packed::register(&c)?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch(
        "PRAGMA foreign_keys=ON; INSERT INTO files_rows VALUES(1);
        WITH RECURSIVE ids(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM ids WHERE n<26)
        INSERT INTO symbols_rows SELECT n FROM ids;
        WITH RECURSIVE ids(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM ids WHERE n<20)
        INSERT INTO facts_rows SELECT n,1 FROM ids;
        INSERT INTO relation_texts VALUES(1,'fixture');",
    )?;
    let mut writer = Writer::new(&c)?;
    for fact in 1..=20 {
        let source = 1 + fact % 2;
        let rows = (3..=26)
            .map(|target| Relation {
                id: (fact - 1) * 24 + target - 2,
                fact,
                source,
                target: Some(target),
                external: None,
                state: Resolution::Ambiguous,
                rule: 1,
                reason: None,
            })
            .collect::<Vec<_>>();
        writer.store(fact, 1, source, &rows)?;
    }
    assert_eq!(
        c.query_row("SELECT count(*) FROM relation_sets", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        c.query_row("SELECT refs FROM relation_sets", [], |r| r.get::<_, i64>(0))?,
        20
    );
    assert_eq!(
        c.query_row("SELECT count(DISTINCT id) FROM relationships", [], |r| r
            .get::<_, i64>(0))?,
        480
    );
    c.execute("DELETE FROM relationship_rows WHERE id=1", [])?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM relation_sets", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM relationships WHERE fact_id=2",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        24
    );
    c.execute_batch("BEGIN; DELETE FROM facts_rows WHERE id=2; ROLLBACK;")?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM relationships", [], |r| r
            .get::<_, i64>(0))?,
        479
    );
    super::super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    assert!(super::super::clear_all(&c).is_err());
    c.execute_batch("BEGIN;")?;
    super::super::clear_all(&c)?;
    for table in [
        "relation_blocks",
        "relation_sets",
        "relation_id_ranges",
        "relation_route_pages",
    ] {
        assert_eq!(
            c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))?,
            0
        );
    }
    c.execute_batch("ROLLBACK;")?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM relationships", [], |r| r
            .get::<_, i64>(0))?,
        479
    );
    super::super::verify(&c).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    c.execute("DELETE FROM files_rows WHERE id=1", [])?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM relation_sets", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM relation_route_pages", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    Ok(())
}
