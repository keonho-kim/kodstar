// Purpose: 관계의 논리 식별·다중도·근거를 임시 SQLite에 모으고 차이를 투영.
// Pattern: Bounded Storage Adapter.
// Usage: graph 비교가 각 체크포인트의 읽기 transaction을 전달.
// Related: crates/kodstar-core/src/application/history/graph.rs, crates/kodstar-core/src/application/history/graph/changes.sql.

use rusqlite::{
    Connection, Row, params_from_iter,
    types::{ToSqlOutput, ValueRef},
};

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, Result},
    history::{GraphEvidence, GraphTarget, HistoryRow},
    results::{QueryRow, QuerySink, QuerySummary, UseSite},
    source::SourceSpan,
};
use crate::infrastructure::cache::paths::{encoding, path_from_bytes};

pub(super) struct Spool {
    connection: Connection,
    remaining: usize,
}

impl Spool {
    pub(super) fn new(connection: Connection, budget: usize) -> Result<Self> {
        connection.execute_batch(
            "CREATE TEMP TABLE edges(side INTEGER, use_key BLOB, endpoint BLOB, state TEXT,
                 count INTEGER, source TEXT, path BLOB, relation TEXT, expression TEXT, rule TEXT,
                 target_path BLOB, target TEXT, external TEXT, start INTEGER, finish INTEGER,
                 line INTEGER, col INTEGER, end_line INTEGER, end_col INTEGER, condition TEXT, type_only INTEGER,
                 UNIQUE(side,use_key,endpoint,state));
             CREATE INDEX by_use ON edges(use_key,endpoint,side,state);",
        )?;
        Ok(Self {
            connection,
            remaining: budget,
        })
    }

    pub(super) fn capture(
        &mut self,
        snapshot: &Connection,
        side: i64,
        cancel: &Cancellation,
    ) -> Result<()> {
        let mut read = snapshot.prepare(
            "SELECT file.path,s.key_blob,f.relation,f.expression,f.condition,f.type_only,
                    s.qualified_name,t.key_blob,dest.path,t.qualified_name,r.external_key,
                    r.resolution,r.rule,f.start_byte,f.end_byte,f.line,f.column_byte,f.end_line,f.end_column_byte
             FROM relationships r CROSS JOIN facts f ON f.id=r.fact_id
             JOIN files_rows file ON file.id=f.file_id JOIN symbols s ON s.id=r.source_id
             LEFT JOIN symbols t ON t.id=r.target_id LEFT JOIN files_rows dest ON dest.id=t.file_id
             WHERE f.relation NOT IN ('defines','contains')",
        )?;
        let transaction = self.connection.transaction()?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO edges VALUES(?1,?2,?3,?4,1,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)
                 ON CONFLICT(side,use_key,endpoint,state) DO UPDATE SET
                     count=edges.count+1,
                     finish=CASE WHEN excluded.start<edges.start THEN excluded.finish ELSE edges.finish END,
                     line=CASE WHEN excluded.start<edges.start THEN excluded.line ELSE edges.line END,
                     col=CASE WHEN excluded.start<edges.start THEN excluded.col ELSE edges.col END,
                     end_line=CASE WHEN excluded.start<edges.start THEN excluded.end_line ELSE edges.end_line END,
                     end_col=CASE WHEN excluded.start<edges.start THEN excluded.end_col ELSE edges.end_col END,
                     rule=CASE WHEN excluded.start<edges.start THEN excluded.rule ELSE edges.rule END,
                     start=MIN(edges.start,excluded.start)",
            )?;
            let mut rows = read.query([])?;
            while let Some(row) = rows.next()? {
                cancel.check()?;
                let use_key = key(row, &[0, 1, 2, 3, 4, 5])?;
                let endpoint = key(row, &[7, 10])?;
                let mut data = [ValueRef::Null; 20];
                data[0] = ValueRef::Integer(side);
                data[1] = ValueRef::Blob(&use_key);
                data[2] = ValueRef::Blob(&endpoint);
                let mut bytes: usize = 256;
                for (slot, field) in data[3..]
                    .iter_mut()
                    .zip([11, 6, 0, 2, 3, 12, 8, 9, 10, 13, 14, 15, 16, 17, 18, 4, 5])
                {
                    *slot = row.get_ref(field)?;
                    if let ValueRef::Text(value) | ValueRef::Blob(value) = slot {
                        bytes = bytes.saturating_add(value.len());
                    }
                }
                self.remaining = self.remaining.checked_sub(bytes).ok_or_else(|| {
                    Error::new(
                        ErrorKind::Limit,
                        "graph diff exceeds the working comparison byte budget",
                    )
                })?;
                insert.execute(params_from_iter(data.map(ToSqlOutput::Borrowed)))?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn publish(
        mut self,
        key: &str,
        report: &super::cache::Comparison,
    ) -> Result<Connection> {
        let value = serde_json::to_string(report)
            .map_err(|error| Error::caused_by("encode graph comparison", error))?;
        let transaction = self.connection.transaction()?;
        transaction.execute_batch("DELETE FROM graph_changes; DELETE FROM graph_evidence;")?;
        transaction.execute_batch(&format!(
            "INSERT INTO graph_changes(kind,before,after)\n{}",
            include_str!("changes.sql")
        ))?;
        transaction.execute_batch(
            "INSERT INTO graph_evidence SELECT rowid,source,path,relation,expression,rule,state,count,
                target_path,target,external,start,finish,line,col,end_line,end_col,condition,type_only
             FROM edges WHERE rowid IN (SELECT before FROM graph_changes WHERE before IS NOT NULL
                                       UNION SELECT after FROM graph_changes WHERE after IS NOT NULL);
             DROP TABLE edges;",
        )?;
        transaction.execute(
            "INSERT OR REPLACE INTO graph_metadata VALUES(1,?1,?2)",
            rusqlite::params![key, value],
        )?;
        transaction.commit()?;
        Ok(self.connection)
    }
}

pub(super) fn emit(
    connection: &Connection,
    limit: Option<usize>,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    summary: &mut QuerySummary,
) -> Result<()> {
    let mut changes =
        connection.prepare("SELECT kind,before,after FROM graph_changes ORDER BY rowid")?;
    let mut evidence = connection.prepare(
            "SELECT source,path,relation,expression,rule,state,count,target_path,target,external,
                    start,finish,line,col,end_line,end_col,condition,type_only FROM graph_evidence WHERE id=?1",
        )?;
    let mut rows = changes.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if limit.is_some_and(|n| summary.result_count >= n as u64) {
            summary.truncate("result_limit");
            break;
        }
        let mut explain = |id: Option<i64>| -> Result<Option<Box<GraphEvidence>>> {
            id.map(|id| {
                let mut records = evidence.query([id])?;
                let row = records
                    .next()?
                    .ok_or_else(|| Error::operation("graph evidence disappeared"))?;
                read_evidence(row).map(Box::new)
            })
            .transpose()
        };
        sink.row(&QueryRow::History(HistoryRow::GraphChange {
            change: row.get(0)?,
            before: explain(row.get(1)?)?,
            after: explain(row.get(2)?)?,
        }))?;
        summary.result_count += 1;
    }
    Ok(())
}

fn key(row: &Row<'_>, fields: &[usize]) -> rusqlite::Result<[u8; 32]> {
    let mut hash = blake3::Hasher::new();
    for &field in fields {
        let value = row.get_ref(field)?;
        match value {
            ValueRef::Text(value) | ValueRef::Blob(value) => {
                hash.update(&[1]);
                hash.update(&(value.len() as u64).to_le_bytes());
                hash.update(value);
            }
            ValueRef::Integer(value) => {
                hash.update(&[2]);
                hash.update(&value.to_le_bytes());
            }
            ValueRef::Null => {
                hash.update(&[0]);
            }
            _ => {
                return Err(rusqlite::Error::InvalidColumnType(
                    field,
                    "graph key".into(),
                    value.data_type(),
                ));
            }
        }
    }
    Ok(*hash.finalize().as_bytes())
}

fn read_evidence(row: &Row<'_>) -> Result<GraphEvidence> {
    let target = match row.get::<_, Option<Vec<u8>>>(7)? {
        Some(raw) => {
            let file = path_from_bytes(&raw)?;
            Some(GraphTarget {
                name: row.get(8)?,
                file: file.to_string_lossy().into_owned(),
                path_encoding: encoding(&file),
            })
        }
        None => None,
    };
    let path = path_from_bytes(&row.get::<_, Vec<u8>>(1)?)?;
    let expression: String = row.get(3)?;
    Ok(GraphEvidence {
        source: row.get(0)?,
        relation: row.get(2)?,
        expression: crate::domain::resolve::java::display(&expression).into_owned(),
        rule: row.get(4)?,
        resolution: row.get(5)?,
        condition: row.get(16)?,
        type_only: row.get(17)?,
        count: row.get(6)?,
        target,
        external: row.get(9)?,
        site: UseSite {
            file: path.to_string_lossy().into_owned(),
            path_encoding: encoding(&path),
            span: SourceSpan {
                start_byte: row.get(10)?,
                end_byte: row.get(11)?,
                line: row.get(12)?,
                column: row.get(13)?,
                end_line: row.get(14)?,
                end_column: row.get(15)?,
            },
        },
    })
}
