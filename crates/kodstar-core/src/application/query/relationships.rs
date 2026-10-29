// Purpose: 관계·파일 연결 행 구성과 sink 전송.
// Pattern: Simple Module.
// Usage: 조회 조정자가 선택한 결과 집합을 전달.
// Related: crates/kodstar-core/src/application/query/mod.rs, crates/kodstar-core/src/application/query/selection.rs.

use rusqlite::{Connection, params};

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::query::QueryOptions;
use crate::domain::relations::Direction;
use crate::domain::results::{FileConnection, QueryRow, QuerySink, QuerySummary};
use crate::infrastructure::cache::paths::{encoding, path_from_bytes};

use super::{reached, traversal::setup};

pub(super) fn stream_relationships(
    connection: &Connection,
    sites: bool,
    limit: Option<usize>,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    summary: &mut QuerySummary,
) -> Result<()> {
    let grouping = if sites {
        "r.id,q.direction"
    } else {
        "r.source_id,r.target_id,r.external_key,f.relation,r.resolution,q.direction,f.condition,f.type_only,r.rule"
    };
    let mut statement=connection.prepare(&format!("SELECT MIN(r.id),q.direction,MIN(q.depth),COUNT(*) FROM q_edges q JOIN relationships r ON r.id=q.id JOIN facts f ON f.id=r.fact_id JOIN symbols s ON s.id=r.source_id JOIN files sf ON sf.id=s.file_id LEFT JOIN symbols t ON t.id=r.target_id LEFT JOIN files tf ON tf.id=t.file_id GROUP BY {grouping} ORDER BY MIN(q.depth),f.relation,COALESCE(tf.path,sf.path),COALESCE(t.line,s.line),COALESCE(t.qualified_name,s.qualified_name),q.direction,MIN(f.start_byte) LIMIT ?1"))?;
    let mut reader = super::relationship_rows::Reader::new(connection)?;
    let mut rows = statement.query([super::sql_limit(limit)])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if reached(limit, summary) {
            break;
        }
        let id: i64 = row.get(0)?;
        let direction: String = row.get(1)?;
        let depth: usize = row.get(2)?;
        let count: u64 = row.get(3)?;
        let relationship = reader.read(
            id,
            if direction == "in" { "in" } else { "out" },
            depth,
            count,
        )?;
        sink.row(&QueryRow::Relationship(Box::new(relationship)))?;
        summary.result_count += 1;
    }
    Ok(())
}

pub(super) fn files(
    connection: &Connection,
    file: i64,
    options: &QueryOptions,
    limit: Option<usize>,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    summary: &mut QuerySummary,
) -> Result<()> {
    setup(connection, options, false)?;
    let direction = match options.direction.unwrap_or(Direction::Both) {
        Direction::Out => "out",
        Direction::In => "in",
        Direction::Both => "both",
    };
    let mut statement = connection.prepare(
        "SELECT other.display_path,other.path,f.relation,'out' AS direction,COUNT(DISTINCT r.id)
         FROM q_allowed r
         JOIN facts f ON f.id=r.fact_id JOIN symbol_identities g ON g.canonical_id=r.target_id JOIN symbols t ON t.id=g.symbol_id
         JOIN files other ON other.id=t.file_id
         WHERE f.file_id=?1 AND t.file_id<>?1 AND ?2 IN ('out','both')
         GROUP BY other.id,f.relation
         UNION ALL
         SELECT other.display_path,other.path,f.relation,'in' AS direction,COUNT(DISTINCT r.id)
         FROM q_allowed r
         JOIN facts f ON f.id=r.fact_id JOIN symbol_identities g ON g.canonical_id=r.target_id JOIN symbols t ON t.id=g.symbol_id
         JOIN files other ON other.id=f.file_id
         WHERE t.file_id=?1 AND f.file_id<>?1 AND ?2 IN ('in','both')
         GROUP BY other.id,f.relation
         ORDER BY 3,2,4 LIMIT ?3",
    )?;
    let mut rows = statement.query(params![file, direction, super::sql_limit(limit)])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if reached(limit, summary) {
            break;
        }
        let raw: Vec<u8> = row.get(1)?;
        let direction: String = row.get(3)?;
        sink.row(&QueryRow::File(FileConnection {
            file: row.get(0)?,
            path_encoding: encoding(&path_from_bytes(&raw)?),
            direction: if direction == "out" { "out" } else { "in" },
            relation: row.get(2)?,
            count: row.get(4)?,
        }))?;
        summary.result_count += 1;
    }
    Ok(())
}
