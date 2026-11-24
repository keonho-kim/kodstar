// Purpose: 제한된 파일 import 그래프에서 순환하는 파일 그룹 산출.
// Pattern: Graph Projection.
// Usage: overview가 확인된 파일과 관계의 임시 테이블을 전달.
// Related: crates/kodstar-core/src/application/query/overview.rs.

use std::collections::BTreeMap;

use petgraph::{algo::kosaraju_scc, graph::DiGraph};
use rusqlite::Connection;

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::Result,
    results::{OverviewPath, OverviewRow},
};
use crate::infrastructure::cache::paths::{encoding, path_from_bytes};

pub(super) fn emit(
    connection: &Connection,
    cancel: &Cancellation,
    output: &mut super::Output<'_>,
) -> Result<()> {
    let mut graph = DiGraph::new();
    let mut nodes = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT DISTINCT source_file,target_file FROM overview_edges
         WHERE source_file<>target_file AND relation IN ('imports','reexports','includes')
         ORDER BY source_file,target_file",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let source = row.get::<_, i64>(0)?;
        let target = row.get::<_, i64>(1)?;
        let from = *nodes
            .entry(source)
            .or_insert_with(|| graph.add_node(source));
        let to = *nodes
            .entry(target)
            .or_insert_with(|| graph.add_node(target));
        graph.add_edge(from, to, ());
    }
    drop(nodes);
    let mut lookup = connection.prepare("SELECT path FROM files_rows WHERE id=?1")?;
    let mut groups = kosaraju_scc(&graph);
    for group in &mut groups {
        cancel.check()?;
        group.sort_unstable_by_key(|node| graph[*node]);
    }
    groups.sort_unstable_by_key(|group| (std::cmp::Reverse(group.len()), graph[group[0]]));
    for group in groups.into_iter().take_while(|group| group.len() > 1) {
        cancel.check()?;
        if !output.ready() {
            break;
        }
        let mut paths = Vec::with_capacity(group.len().min(32));
        for node in group.iter().take(32) {
            let raw: Vec<u8> = lookup.query_row([graph[*node]], |row| row.get(0))?;
            let path = path_from_bytes(&raw)?;
            paths.push(OverviewPath {
                path: path.to_string_lossy().into_owned(),
                path_encoding: encoding(&path),
            });
        }
        output.emit(OverviewRow::ImportCycle {
            files: group.len(),
            sample: paths,
        })?;
    }
    Ok(())
}
