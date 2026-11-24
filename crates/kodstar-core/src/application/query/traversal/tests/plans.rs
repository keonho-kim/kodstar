// Purpose: 이웃 조회가 무관한 전체 정점·파일 view를 복원하지 않는지 검증.
// Pattern: Query Plan Regression Test.
// Usage: traversal 테스트가 1,600개 정점에서 실제 SQL의 scan 수를 확인.
// Related: crates/kodstar-core/src/infrastructure/index/adjacency.rs, crates/kodstar-core/src/application/query/traversal/disk.rs.

use super::*;
use crate::application::query::traversal::graph::Orientation;
use rusqlite::StatementStatus;

#[test]
fn neighbor_lookup_does_not_materialize_all_symbol_rows() -> Result<()> {
    let c = fixture(1600)?;
    for (direction, id) in [(Orientation::Out, 2), (Orientation::In, 4)] {
        let query = disk::neighbors_query(direction);
        let mut statement = c.prepare(&query)?;
        let mut rows = statement.query([id])?;
        let mut count = 0;
        while rows.next()?.is_some() {
            count += 1;
        }
        drop(rows);
        assert!(count > 0);
        assert!(
            statement.get_status(StatementStatus::FullscanStep) < 20,
            "a neighbor lookup must not rescan all 1,600 unrelated symbol rows"
        );
    }
    Ok(())
}
