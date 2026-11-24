// Purpose: 한 인덱스 세대의 모듈·관계·주요 객체를 제한된 범위에서 집계.
// Pattern: Streaming Query.
// Usage: query 조정자가 Overview 요청을 같은 읽기 transaction에서 실행.
// Related: crates/kodstar-core/src/application/query/overview/cycles.rs, crates/kodstar-core/src/application/query/relationship_rows.rs.

mod cycles;

use std::path::Path;

use rusqlite::{Connection, params};

use crate::application::workspace::Settings;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::Result,
    query::QueryOptions,
    results::{OverviewPath, OverviewRow, QueryRow, QuerySink, QuerySummary},
};
use crate::infrastructure::{
    cache::paths::{encoding, path_bytes, path_from_bytes},
    index,
};

pub(super) fn run(
    connection: &Connection,
    settings: &Settings,
    options: &QueryOptions,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    summary: &mut QuerySummary,
) -> Result<()> {
    let maximum = options
        .max_nodes
        .resolve(settings.max_nodes)
        .unwrap_or(usize::MAX);
    let budget = settings.memory_budget / 512;
    let files = maximum.min(budget);
    let edges = budget;
    let limit = options.limit.resolve(settings.limit);
    connection.execute_batch(
        "CREATE TEMP TABLE overview_files(id INTEGER PRIMARY KEY,module BLOB NOT NULL,package INTEGER);
         CREATE TEMP TABLE overview_edges(id INTEGER PRIMARY KEY,src INTEGER,dst INTEGER,
               source_file INTEGER,target_file INTEGER,relation TEXT);",
    )?;
    let all_files = read_files(connection, files, cancel, summary)?;
    let mut result = Output {
        limit,
        sink,
        summary,
    };
    if modules(connection, cancel, &mut result)? {
        read_edges(connection, all_files, edges, result.summary)?;
        if dependencies(connection, cancel, &mut result)? && hubs(connection, cancel, &mut result)?
        {
            cycles::emit(connection, cancel, &mut result)?;
        }
    }
    Ok(())
}

fn read_files(
    connection: &Connection,
    maximum: usize,
    cancel: &Cancellation,
    summary: &mut QuerySummary,
) -> Result<bool> {
    let mut statement = connection.prepare(
        "SELECT id,path,package_id FROM files WHERE status IN ('analyzed','auxiliary') ORDER BY path LIMIT ?1",
    )?;
    let mut insert = connection.prepare("INSERT INTO overview_files VALUES(?1,?2,?3)")?;
    let mut rows = statement.query([super::sql_limit(Some(maximum))])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if summary.visited_count >= maximum as u64 {
            summary.truncate("overview_file_limit");
            return Ok(false);
        }
        let path = path_from_bytes(&row.get::<_, Vec<u8>>(1)?)?;
        let module = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        insert.execute(params![
            row.get::<_, i64>(0)?,
            path_bytes(module),
            row.get::<_, Option<i64>>(2)?
        ])?;
        summary.visited_count += 1;
    }
    Ok(true)
}

fn read_edges(
    connection: &Connection,
    all_files: bool,
    maximum: usize,
    summary: &mut QuerySummary,
) -> Result<()> {
    let sources = if all_files {
        "relationships r"
    } else {
        "(SELECT DISTINCT block.source_id FROM overview_files f
          JOIN relation_blocks block ON block.file_id=f.id) s
         CROSS JOIN relationships r ON r.source_id=s.source_id"
    };
    let inserted = connection.execute(
        &format!(
            "INSERT INTO overview_edges
             SELECT r.id,r.source_id,r.target_id,a.id,b.id,f.relation
             FROM {sources} CROSS JOIN facts f ON f.id=r.fact_id
             CROSS JOIN overview_files a ON a.id=f.file_id
             CROSS JOIN symbols_rows t ON t.id=r.target_id
             CROSS JOIN overview_files b ON b.id=t.file_id
             WHERE r.resolution='resolved' AND f.relation NOT IN ('defines','contains') LIMIT ?1"
        ),
        [super::sql_limit(Some(maximum))],
    )?;
    if inserted > maximum {
        summary.truncate("overview_edge_limit");
    }
    Ok(())
}

fn modules(
    connection: &Connection,
    cancel: &Cancellation,
    output: &mut Output<'_>,
) -> Result<bool> {
    let mut statement = connection
        .prepare("SELECT module,COUNT(*) FROM overview_files GROUP BY module ORDER BY module")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if !output.ready() {
            return Ok(false);
        }
        let path = path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        output.emit(OverviewRow::Module {
            path: path.to_string_lossy().into_owned(),
            path_encoding: encoding(&path),
            files: row.get(1)?,
        })?;
    }
    Ok(true)
}

fn dependencies(
    connection: &Connection,
    cancel: &Cancellation,
    output: &mut Output<'_>,
) -> Result<bool> {
    let mut statement = connection.prepare(
        "WITH boundaries AS (
            SELECT 0 AS kind,NULL AS from_package,NULL AS to_package,e.relation,
                   COUNT(*) AS count,MIN(e.id) AS witness
            FROM overview_edges e JOIN overview_files a ON a.id=e.source_file
            JOIN overview_files b ON b.id=e.target_file WHERE a.module<>b.module
            GROUP BY a.module,b.module,e.relation
            UNION ALL
            SELECT 1,a.package,b.package,e.relation,COUNT(*),MIN(e.id)
            FROM overview_edges e JOIN overview_files a ON a.id=e.source_file
            JOIN overview_files b ON b.id=e.target_file WHERE a.package<>b.package
            GROUP BY a.package,b.package,e.relation
         )
         SELECT a.module,b.module,g.relation,g.count,g.witness,p.value,t.value
         FROM boundaries g JOIN overview_edges e ON e.id=g.witness
         JOIN overview_files a ON a.id=e.source_file JOIN overview_files b ON b.id=e.target_file
         LEFT JOIN atoms p ON p.id=g.from_package LEFT JOIN atoms t ON t.id=g.to_package
         ORDER BY g.kind,g.count DESC,a.module,b.module,p.value,t.value,g.relation",
    )?;
    let mut rows = statement.query([])?;
    let mut reader = super::relationship_rows::Reader::new(connection)?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if !output.ready() {
            return Ok(false);
        }
        let from = path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        let to = path_from_bytes(&row.get::<_, Vec<u8>>(1)?)?;
        let edge = reader.read(row.get(4)?, "out", 1, 1)?;
        let relation = row.get(2)?;
        let count = row.get(3)?;
        let site = edge.site;
        if let (Some(from_package), Some(to_package)) = (row.get(5)?, row.get(6)?) {
            output.emit(OverviewRow::PackageDependency {
                from_package,
                to_package,
                relation,
                count,
                site,
                from: OverviewPath {
                    path: from.to_string_lossy().into_owned(),
                    path_encoding: encoding(&from),
                },
                to: OverviewPath {
                    path: to.to_string_lossy().into_owned(),
                    path_encoding: encoding(&to),
                },
            })?;
        } else {
            output.emit(OverviewRow::ModuleDependency {
                from: from.to_string_lossy().into_owned(),
                from_encoding: encoding(&from),
                to: to.to_string_lossy().into_owned(),
                to_encoding: encoding(&to),
                relation,
                count,
                site,
            })?;
        }
    }
    Ok(true)
}

fn hubs(connection: &Connection, cancel: &Cancellation, output: &mut Output<'_>) -> Result<bool> {
    let mut statement = connection.prepare(
        "WITH totals AS (
           SELECT dst,COUNT(DISTINCT src) incoming FROM overview_edges WHERE src<>dst GROUP BY dst
         )
         SELECT s.id,t.incoming FROM totals t JOIN symbols s ON s.id=t.dst
         JOIN files f ON f.id=s.file_id WHERE s.kind<>'file'
         ORDER BY t.incoming DESC,f.path,s.line,s.column_byte,s.qualified_name,s.key_blob",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if !output.ready() {
            return Ok(false);
        }
        output.emit(OverviewRow::Hub {
            symbol: Box::new(index::symbol(connection, row.get(0)?)?),
            incoming: row.get(1)?,
        })?;
    }
    Ok(true)
}

struct Output<'a> {
    limit: Option<usize>,
    sink: &'a mut dyn QuerySink,
    summary: &'a mut QuerySummary,
}

impl Output<'_> {
    fn ready(&mut self) -> bool {
        !super::reached(self.limit, self.summary)
    }

    fn emit(&mut self, row: OverviewRow) -> Result<()> {
        self.sink.row(&QueryRow::Overview(Box::new(row)))?;
        self.summary.result_count += 1;
        Ok(())
    }
}
