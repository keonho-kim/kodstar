// Purpose: 체크포인트 선택·과거 파일 구성과 이름의 저장 계약.
// Pattern: Storage Adapter.
// Usage: 이력 명령과 Snapshot이 저장된 상태를 선택.
// Related: crates/kodstar-core/src/infrastructure/history/schema.sql.

use crate::domain::diagnostics::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn display(id: i64) -> String {
    format!("c{id}")
}

pub(crate) fn head(connection: &Connection) -> Result<Option<i64>> {
    Ok(connection.query_row(
        "SELECT b.checkpoint FROM branches b JOIN repository r ON b.name=r.current_branch",
        [],
        |r| r.get(0),
    )?)
}

pub(crate) fn resolve(connection: &Connection, revision: &str) -> Result<i64> {
    let numeric = revision
        .strip_prefix('c')
        .and_then(|id| id.parse::<i64>().ok())
        .filter(|id| *id > 0);
    let id = if let Some(id) = numeric {
        connection
            .query_row("SELECT id FROM checkpoints WHERE id=?1", [id], |r| r.get(0))
            .optional()?
    } else {
        connection
            .query_row(
                "SELECT checkpoint FROM names WHERE name=?1",
                [revision],
                |r| r.get(0),
            )
            .optional()?
    };
    id.ok_or_else(|| Error::not_found(format!("checkpoint not found: {revision}")))
}

pub(crate) fn materialize(connection: &Connection, id: i64) -> Result<()> {
    connection.execute_batch("DROP TABLE IF EXISTS temp.snapshot_files; CREATE TEMP TABLE snapshot_files(path BLOB PRIMARY KEY,blob INTEGER,mode INTEGER,kind TEXT) WITHOUT ROWID;")?;
    if head(connection)? == Some(id) {
        connection
            .execute_batch("INSERT INTO snapshot_files SELECT path,blob,mode,kind FROM working;")?;
        return Ok(());
    }
    connection.execute("WITH RECURSIVE ancestry(id) AS (SELECT ?1 UNION SELECT c.parent FROM checkpoints c JOIN ancestry a ON c.id=a.id WHERE c.parent IS NOT NULL), latest AS (SELECT e.path,max(e.checkpoint) checkpoint FROM entries e JOIN ancestry a ON a.id=e.checkpoint GROUP BY e.path) INSERT INTO snapshot_files SELECT e.path,e.blob,e.mode,e.kind FROM entries e JOIN latest l ON l.path=e.path AND l.checkpoint=e.checkpoint WHERE e.blob IS NOT NULL", [id])?;
    Ok(())
}

pub(crate) fn label(connection: &Connection, id: i64, label: &str) -> Result<()> {
    crate::domain::history::validate_name(label)?;
    let previous: Option<i64> = connection
        .query_row("SELECT checkpoint FROM names WHERE name=?1", [label], |r| {
            r.get(0)
        })
        .optional()?;
    if previous == Some(id) {
        return Ok(());
    }
    if previous.is_some() {
        return Err(Error::argument(
            "checkpoint name already refers to another state",
        ));
    }
    let named: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM names WHERE checkpoint=?1)",
        [id],
        |r| r.get(0),
    )?;
    if named {
        return Err(Error::argument(
            "checkpoint already has a name; preserving the existing name",
        ));
    }
    connection.execute("INSERT INTO names VALUES(?1,?2)", params![label, id])?;
    Ok(())
}
