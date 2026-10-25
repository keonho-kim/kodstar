// Purpose: 분석 행의 atom 참조 존재·정확한 계수·수명 제약 구성.
// Pattern: Transactional Reference Counting.
// Usage: records 생성이 물리 테이블별 작은 trigger를 등록.
// Related: crates/kodstar-core/src/infrastructure/index/records/mod.rs, crates/kodstar-core/src/infrastructure/index/atoms/schema.sql.

use super::Layout;
use rusqlite::{Connection, Result};

pub(super) fn create(c: &Connection, l: &Layout) -> Result<()> {
    let table = format!("{}_rows", l.name);
    let ids = |prefix: &str| {
        l.atoms
            .iter()
            .map(|k| format!("{prefix}.{k}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let count = |prefix: &str| {
        l.atoms
            .iter()
            .map(|k| format!("CASE WHEN id={prefix}.{k} THEN 1 ELSE 0 END"))
            .collect::<Vec<_>>()
            .join("+")
    };
    let changed = l
        .atoms
        .iter()
        .map(|k| format!("NEW.{k} IS NOT OLD.{k}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let values = l
        .atoms
        .iter()
        .map(|k| format!("SELECT NEW.{k} AS id"))
        .collect::<Vec<_>>()
        .join(" UNION ");
    let guard = format!(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM ({values}) pending WHERE id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM atoms WHERE atoms.id=pending.id)) THEN RAISE(ABORT,'missing analysis string atom') END;"
    );
    let new_count = count("NEW");
    let old_count = count("OLD");
    c.execute_batch(&format!("
        CREATE TRIGGER {table}_atoms_update_guard BEFORE UPDATE ON {table} WHEN {changed} BEGIN {guard} END;
        CREATE TRIGGER {table}_atoms_insert AFTER INSERT ON {table} BEGIN
          UPDATE atoms SET refs=refs+({new_count}) WHERE id IN ({new_ids});
          SELECT CASE WHEN changes()<>(SELECT count(*) FROM ({values}) pending WHERE id IS NOT NULL)
            THEN RAISE(ABORT,'missing analysis string atom') END; END;
        CREATE TRIGGER {table}_atoms_delete AFTER DELETE ON {table} BEGIN
          UPDATE atoms SET refs=refs-({old_count}) WHERE id IN ({old_ids}); END;
        CREATE TRIGGER {table}_atoms_update AFTER UPDATE ON {table} WHEN {changed} BEGIN
          UPDATE atoms SET refs=refs+({new_count})-({old_count}) WHERE id IN ({new_ids},{old_ids}) AND ({new_count})<>({old_count}); END;
    ",new_ids=ids("NEW"),old_ids=ids("OLD")))?;
    Ok(())
}
