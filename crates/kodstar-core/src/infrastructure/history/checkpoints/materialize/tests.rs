// Purpose: 긴 이력의 근접 시점과 분기 상태를 제한 작업량과 기준 재구성으로 대조.
// Pattern: Differential Storage Test.
// Usage: materialize가 실제 SQLite 메타데이터와 progress 예산을 사용.
// Related: crates/kodstar-core/src/infrastructure/history/checkpoints/materialize.rs.

use super::*;

fn fixture() -> Result<Connection> {
    let c = Connection::open_in_memory()?;
    c.execute_batch(include_str!("../../schema.sql"))?;
    c.execute_batch(include_str!("../../metadata.sql"))?;
    c.execute_batch("INSERT INTO blobs VALUES(1,zeroblob(32),0),(2,randomblob(32),0);
      WITH RECURSIVE ids(i) AS(VALUES(1) UNION ALL SELECT i+1 FROM ids WHERE i<20000)
      INSERT INTO checkpoints SELECT i,nullif(i-1,0),0,CASE WHEN i=1 THEN 100 ELSE 1 END,100 FROM ids;
      UPDATE branches SET checkpoint=20000;
      WITH RECURSIVE ids(i) AS(VALUES(1) UNION ALL SELECT i+1 FROM ids WHERE i<100)
      INSERT INTO paths SELECT i,CAST('file-'||i AS BLOB) FROM ids;
      INSERT INTO entry_rows SELECT 1,id,CASE WHEN id=1 THEN 2 ELSE 1 END,420,NULL,NULL FROM paths;
      INSERT INTO entry_rows SELECT id,1,id%2+1,420,2-id%2,420 FROM checkpoints WHERE id>1;
      INSERT INTO working SELECT p.value,CASE WHEN p.id=1 THEN 1 ELSE 1 END,420,'file',0,'','' FROM paths p;")?;
    Ok(c)
}

#[test]
fn nearby_revision_does_not_walk_twenty_thousand_ancestors() -> Result<()> {
    let c = fixture()?;
    let mut calls = 0;
    c.progress_handler(
        1000,
        Some(move || {
            calls += 1;
            calls > 20
        }),
    )?;
    run(&c, 19999)?;
    c.progress_handler(0, None::<fn() -> bool>)?;
    assert_eq!(
        c.query_row(
            "SELECT blob FROM snapshot_files WHERE path=CAST('file-1' AS BLOB)",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        2
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM snapshot_files", [], |r| r
            .get::<_, i64>(0))?,
        100
    );
    Ok(())
}

#[test]
fn branches_and_distant_revisions_match_forward_reconstruction() -> Result<()> {
    let c = fixture()?;
    c.execute_batch("INSERT INTO checkpoints VALUES(20001,19995,0,1,100);
        INSERT INTO entry_rows VALUES(20001,1,1,420,2,420);
        CREATE TEMP TABLE expected(path BLOB PRIMARY KEY,blob INTEGER,mode INTEGER,kind TEXT) WITHOUT ROWID;")?;
    for id in [20001, 19995, 100, 1] {
        run(&c, id)?;
        c.execute("WITH RECURSIVE ancestry(id) AS(SELECT ?1 UNION SELECT c.parent FROM checkpoints c JOIN ancestry a ON a.id=c.id WHERE c.parent IS NOT NULL),
          latest AS(SELECT e.path,max(e.checkpoint) checkpoint FROM entries e JOIN ancestry a ON a.id=e.checkpoint GROUP BY e.path)
          INSERT INTO expected SELECT e.path,e.blob,e.mode,e.kind FROM entries e JOIN latest l ON e.path=l.path AND e.checkpoint=l.checkpoint WHERE e.blob IS NOT NULL", [id])?;
        assert!(!c.query_row("SELECT EXISTS(SELECT * FROM expected EXCEPT SELECT * FROM snapshot_files) OR EXISTS(SELECT * FROM snapshot_files EXCEPT SELECT * FROM expected)", [], |r| r.get::<_,bool>(0))?, "{id}");
        c.execute("DELETE FROM expected", [])?;
    }
    Ok(())
}
