// Purpose: 식별자·한정 이름·경로의 단어로 조회 후보와 직접 관계 선택.
// Pattern: Streaming Query.
// Usage: query 조정자가 RankedSearch와 같은 인덱스 세대의 연결을 전달.
// Related: crates/kodstar-core/src/application/query/search.rs, crates/kodstar-core/src/application/query/traversal.rs.

use std::cmp::Reverse;
use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use crate::application::workspace::Settings;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    query::{QueryOptions, terms::words},
    relations::Direction,
    results::{QueryRow, QuerySink, QuerySummary},
};
use crate::infrastructure::index;

const SEEDS: usize = 8;

pub(super) fn run(
    connection: &Connection,
    text: &str,
    settings: &Settings,
    options: &QueryOptions,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    summary: &mut QuerySummary,
) -> Result<()> {
    let terms = words(text);
    if terms.is_empty() {
        return Err(Error::argument("ranked search requires a letter or digit"));
    }
    let maximum = options.max_nodes.resolve(settings.max_nodes);
    let limit = options.limit.resolve(settings.limit);
    let mut candidates = BTreeSet::new();
    connection.execute_batch(
        "CREATE TEMP TABLE q_rank_terms(lo TEXT PRIMARY KEY,hi TEXT NOT NULL) WITHOUT ROWID;",
    )?;
    for term in &terms {
        if let Some(upper) = super::search::upper_bound(term) {
            connection.execute(
                "INSERT OR IGNORE INTO q_rank_terms VALUES(?1,?2)",
                params![term, upper],
            )?;
        }
    }
    let mut statement = connection.prepare(
        "WITH matched(id) AS (
             SELECT t.id FROM q_rank_terms q CROSS JOIN ranked_symbol_terms t
               ON t.term>=q.lo AND t.term<q.hi
             UNION
             SELECT s.id FROM q_rank_terms q CROSS JOIN ranked_file_terms t
               ON t.term>=q.lo AND t.term<q.hi JOIN symbols_rows s ON s.file_id=t.id
         )
         SELECT s.id,s.name,s.qualified_name,f.id,f.display_path
         FROM matched m CROSS JOIN symbols s ON s.id=m.id JOIN files f ON f.id=s.file_id
         WHERE s.kind<>'file' AND NOT EXISTS(SELECT 1 FROM symbol_aliases a WHERE a.symbol_id=s.id)
         ORDER BY f.path,s.start_byte,s.qualified_name,s.key_blob,s.id LIMIT ?1",
    )?;
    let mut rows = statement.query([super::sql_limit(maximum)])?;
    let mut current_path = None::<(i64, String, Vec<String>)>;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if maximum.is_some_and(|n| summary.visited_count >= n as u64) {
            summary.truncate("search_scan_limit");
            break;
        }
        let order = summary.visited_count;
        summary.visited_count += 1;
        let name: String = row.get(1)?;
        let qualified: String = row.get(2)?;
        let file = row.get(3)?;
        if current_path.as_ref().is_none_or(|(id, _, _)| *id != file) {
            let path: String = row.get(4)?;
            let path_terms = words(&path);
            current_path = Some((file, path, path_terms));
        }
        let (_, path, path_terms) = current_path
            .as_ref()
            .ok_or_else(|| Error::operation("missing ranked path"))?;
        let score = score(text, &terms, &name, &qualified, path, path_terms);
        if score.0 > 0 {
            candidates.insert((Reverse(score), order, row.get::<_, i64>(0)?));
            if candidates.len() > SEEDS {
                candidates.pop_last();
            }
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }
    super::traversal::setup(connection, options, false)?;
    for (_, _, id) in candidates {
        cancel.check()?;
        if super::reached(limit, summary) {
            return Ok(());
        }
        sink.row(&QueryRow::Symbol(index::symbol(connection, id)?))?;
        summary.result_count += 1;
        connection.execute("INSERT INTO q_seeds VALUES(?1)", [id])?;
    }
    select_context(connection, options, settings.memory_budget / 256, summary)?;
    super::relationships::stream_relationships(
        connection,
        options.sites,
        limit,
        cancel,
        sink,
        summary,
    )
}

fn select_context(
    connection: &Connection,
    options: &QueryOptions,
    maximum: usize,
    summary: &mut QuerySummary,
) -> Result<()> {
    let direction = options.direction.unwrap_or(Direction::Both);
    for (allowed, side, column) in [
        (direction != Direction::In, "out", "source_id"),
        (direction != Direction::Out, "in", "target_id"),
    ] {
        if !allowed {
            continue;
        }
        let count = connection.execute(
            &format!(
                "INSERT OR IGNORE INTO q_edges
                 SELECT r.id,?1,1 FROM q_seeds q CROSS JOIN q_allowed r ON r.{column}=q.id LIMIT ?2"
            ),
            params![side, super::sql_limit(Some(maximum))],
        )?;
        if count > maximum {
            summary.truncate("search_context_limit");
        }
    }
    Ok(())
}

fn score(
    text: &str,
    terms: &[String],
    name: &str,
    qualified: &str,
    path: &str,
    path_terms: &[String],
) -> (usize, u8, usize) {
    let exact = if qualified.eq_ignore_ascii_case(text) {
        3
    } else if name.eq_ignore_ascii_case(text) {
        2
    } else {
        u8::from(path.eq_ignore_ascii_case(text))
    };
    let qualified_words = (name != qualified).then(|| words(qualified));
    let name = words(name);
    let qualified = qualified_words.as_deref().unwrap_or(&name);
    let mut matches = 0;
    let mut weight = 0;
    for term in terms {
        let strength = if name.contains(term) {
            5
        } else if qualified.contains(term) {
            4
        } else if path_terms.contains(term) {
            3
        } else if name.iter().any(|word| word.starts_with(term)) {
            2
        } else {
            usize::from(
                qualified
                    .iter()
                    .chain(path_terms)
                    .any(|word| word.starts_with(term)),
            )
        };
        if strength > 0 {
            matches += 1;
            weight += strength;
        }
    }
    (matches, exact, weight)
}
