// Purpose: 언어별 이름 키와 Unicode 경계로 정확·prefix 검색.
// Pattern: Simple Module.
// Usage: query 조정자가 Search 요청과 출력 한도를 전달.
// Related: crates/kodstar-core/src/application/query/mod.rs, crates/kodstar-core/src/application/query/selection.rs.

use super::selection::stream_symbols;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::resolve::lisp;
use crate::domain::results::{QuerySink, QuerySummary};
use rusqlite::Connection;

#[cfg(test)]
mod tests;

pub(super) fn run(
    c: &Connection,
    name: &str,
    prefix: bool,
    limit: Option<usize>,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    summary: &mut QuerySummary,
) -> Result<()> {
    let pascal = name.to_ascii_lowercase();
    let lisp = lisp::key(name);
    let rust = crate::domain::languages::Language::Rust.lookup_key(name);
    if prefix {
        let (normal_end, pascal_end, lisp_end) =
            (upper_bound(name), upper_bound(&pascal), upper_bound(&lisp));
        let rust_end = upper_bound(&rust);
        let ranges = [
            (1, 4, &normal_end),
            (2, 5, &pascal_end),
            (3, 6, &lisp_end),
            (7, 8, &rust_end),
        ]
        .into_iter()
        .map(|(start, end, bound)| {
            if bound.is_some() {
                format!("SELECT id FROM atoms WHERE value>=?{start} AND value<?{end}")
            } else {
                format!("SELECT id FROM atoms WHERE value>=?{start}")
            }
        })
        .collect::<Vec<_>>()
        .join(" UNION ");
        let predicate = format!("s.name_key_id IN ({ranges}) AND (
            (s.language NOT IN ('pascal','commonlisp','rust') AND s.name_key>=?1 AND (?4 IS NULL OR s.name_key<?4))
            OR (s.language='pascal' AND s.name_key>=?2 AND (?5 IS NULL OR s.name_key<?5))
            OR (s.language='commonlisp' AND s.name_key>=?3 AND (?6 IS NULL OR s.name_key<?6))
            OR (s.language='rust' AND s.name_key>=?7 AND (?8 IS NULL OR s.name_key<?8)))");
        stream_symbols(
            c,
            &predicate,
            &[
                &name,
                &pascal,
                &lisp,
                &normal_end,
                &pascal_end,
                &lisp_end,
                &rust,
                &rust_end,
            ],
            limit,
            cancel,
            sink,
            summary,
        )
    } else {
        stream_symbols(
            c,
            "s.name_key_id IN (SELECT id FROM atoms WHERE value IN (?1,?2,?3,?4)) AND (
                (s.language NOT IN ('pascal','commonlisp','rust') AND s.name_key=?1)
                OR (s.language='pascal' AND s.name_key=?2)
                OR (s.language='commonlisp' AND s.name_key=?3)
                OR (s.language='rust' AND s.name_key=?4))",
            &[&name, &pascal, &lisp, &rust],
            limit,
            cancel,
            sink,
            summary,
        )
    }
}

fn upper_bound(prefix: &str) -> Option<String> {
    let mut chars: Vec<_> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        let mut scalar = u32::from(last) + 1;
        while scalar <= 0x10ffff {
            if let Some(next) = char::from_u32(scalar) {
                chars.push(next);
                return Some(chars.into_iter().collect());
            }
            scalar += 1;
        }
    }
    None
}
