// Purpose: 불변 체크포인트 쌍의 최근 그래프 비교와 근거를 원자적으로 보존.
// Pattern: Bounded Storage Adapter.
// Usage: 이력 비교가 프로젝트 cache writer 아래 읽고 게시.
// Related: crates/kodstar-core/src/application/history/graph.rs, crates/kodstar-core/src/application/history/graph/edges.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Coverage, Error, Result},
};
use crate::infrastructure::index;
use rusqlite::{Connection, OptionalExtension};
use std::{fs, path::Path, time::Duration};

const APPLICATION: u32 = 0x4b534744;
const VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Comparison {
    pub generation: i64,
    pub indexed_at: u64,
    pub before: Option<Coverage>,
    pub after: Coverage,
}

pub(super) fn open(directory: &Path, budget: usize, cancel: &Cancellation) -> Result<Connection> {
    for name in [
        "graph-diff.sqlite",
        "graph-diff.sqlite-wal",
        "graph-diff.sqlite-shm",
    ] {
        let path = directory.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                return Err(Error::operation(
                    "graph comparison cache must be a regular file",
                ));
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(Error::io("inspect graph comparison cache", &path, error));
            }
            _ => {}
        }
    }
    let mut connection = Connection::open(directory.join("graph-diff.sqlite"))?;
    connection.busy_timeout(Duration::from_secs(5))?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let application: u32 = connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
    if (version, application) == (0, 0) {
        let empty =
            connection.query_row("SELECT NOT EXISTS(SELECT 1 FROM sqlite_schema)", [], |r| {
                r.get::<_, bool>(0)
            })?;
        if !empty {
            return Err(Error::operation("unrecognized graph comparison cache"));
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE graph_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1), key TEXT NOT NULL, report TEXT NOT NULL);
             CREATE TABLE graph_changes(kind TEXT NOT NULL,before INTEGER,after INTEGER);
             CREATE TABLE graph_evidence(
                id INTEGER PRIMARY KEY,source TEXT,path BLOB,relation TEXT,expression TEXT,
                rule TEXT,state TEXT,count INTEGER,target_path BLOB,target TEXT,external TEXT,
                start INTEGER,finish INTEGER,line INTEGER,col INTEGER,end_line INTEGER,end_col INTEGER,
                condition TEXT,type_only INTEGER
             );",
        )?;
        transaction.pragma_update(None, "user_version", VERSION)?;
        transaction.pragma_update(None, "application_id", APPLICATION)?;
        transaction.commit()?;
    } else if (version, application) != (VERSION, APPLICATION) {
        return Err(Error::operation(
            "incompatible graph comparison cache; run kodstar cache clean",
        ));
    }
    connection.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA cache_size=-256; PRAGMA temp_store=FILE;",
    )?;
    index::limit_reads(&connection, budget)?;
    index::observe_cancellation(&connection, cancel)?;
    Ok(connection)
}

pub(super) fn read(connection: &Connection, key: &str) -> Result<Option<Comparison>> {
    let report: Option<String> = connection
        .query_row(
            "SELECT report FROM graph_metadata WHERE singleton=1 AND key=?1",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    report
        .map(|report| {
            serde_json::from_str(&report)
                .map_err(|e| Error::caused_by("decode graph comparison cache", e))
        })
        .transpose()
}
