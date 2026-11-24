// Purpose: 독립된 식이 아닌 Erlang macro 치환 token을 입력 역할에 따라 분리.
// Pattern: Source Adapter.
// Usage: parser가 실패 tree와 원문을 전달하고 위치를 보존한 재분석 입력을 받음.
// Related: crates/kodstar-core/src/infrastructure/parse/input/native_session.rs, crates/kodstar-core/src/infrastructure/parse/session.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use std::ops::Range;
use tree_sitter::Tree;

pub(super) fn fragments(
    tree: &Tree,
    lines: &mut Vec<usize>,
    budget: usize,
    cancel: &Cancellation,
) -> Result<()> {
    let mut cursor = tree.root_node().walk();
    loop {
        cancel.check()?;
        let node = cursor.node();
        if matches!(node.kind(), "macro_case_expr" | "macro_catch_clause") {
            lines.push(node.start_position().row + 1);
            if lines.len() * 16 > budget {
                return Err(Error::input(
                    "source_analysis_limit",
                    "Erlang fragment workspace exceeded",
                    None,
                ));
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                lines.sort_unstable();
                lines.dedup();
                return Ok(());
            }
        }
    }
}

pub(super) fn prepare(
    tree: &Tree,
    source: &[u8],
    budget: usize,
    cancel: &Cancellation,
) -> Result<Option<(Vec<u8>, Vec<usize>)>> {
    let mut errors = Vec::new();
    let mut cursor = tree.root_node().walk();
    loop {
        cancel.check()?;
        let node = cursor.node();
        let mut child = node.walk();
        if (node.is_error() || node.is_missing())
            && !node
                .children(&mut child)
                .any(|n| n.is_error() || n.is_missing() || n.has_error())
        {
            errors.push(node.start_byte());
        }
        if errors.len() * 16 > budget {
            return Err(Error::input(
                "source_analysis_limit",
                "Erlang fragment workspace exceeded",
                None,
            ));
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return project(source, &errors, budget, cancel);
            }
        }
    }
}

fn project(
    source: &[u8],
    errors: &[usize],
    budget: usize,
    cancel: &Cancellation,
) -> Result<Option<(Vec<u8>, Vec<usize>)>> {
    let mut at = 0;
    let mut masked = None;
    let mut lines = Vec::new();
    while at < source.len() {
        cancel.check()?;
        if let Some(end) = literal(source, at) {
            at = end;
            continue;
        }
        if source[at..].starts_with(b"-define")
            && let Some(range) = replacement(source, at + 7)
        {
            at = range.end + 1;
            if errors.iter().any(|p| range.contains(p) || *p == range.end) {
                let output = masked.get_or_insert_with(|| source.to_vec());
                for byte in &mut output[range.clone()] {
                    if !matches!(byte, b'\n' | b'\r') {
                        *byte = b' ';
                    }
                }
                lines.push(memchr::memchr_iter(b'\n', &source[..range.start]).count() + 1);
                if lines.len() * 16 > budget {
                    return Err(Error::input(
                        "source_analysis_limit",
                        "Erlang fragment workspace exceeded",
                        None,
                    ));
                }
            }
            continue;
        }
        at += 1;
    }
    Ok(masked.map(|bytes| (bytes, lines)))
}

fn replacement(source: &[u8], mut at: usize) -> Option<Range<usize>> {
    while source.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if source.get(at) != Some(&b'(') {
        return None;
    }
    let mut depth = 1usize;
    let mut begin = None;
    at += 1;
    while at < source.len() {
        if let Some(end) = literal(source, at) {
            at = end;
            continue;
        }
        match source[at] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let mut end = at + 1;
                    while source.get(end).is_some_and(u8::is_ascii_whitespace) {
                        end += 1;
                    }
                    return (source.get(end) == Some(&b'.')).then_some(begin?..at);
                }
            }
            b',' if depth == 1 && begin.is_none() => begin = Some(at + 1),
            _ => {}
        }
        at += 1;
    }
    None
}

fn literal(source: &[u8], at: usize) -> Option<usize> {
    let byte = *source.get(at)?;
    if byte == b'%' {
        return Some(at + memchr::memchr(b'\n', &source[at..]).unwrap_or(source.len() - at));
    }
    if byte == b'$' {
        let end = at
            + if source.get(at + 1) == Some(&b'\\') {
                3
            } else {
                2
            };
        return Some(end.min(source.len()));
    }
    if !matches!(byte, b'\'' | b'"') {
        return None;
    }
    let mut end = at + 1;
    while end < source.len() {
        if source[end] == byte {
            return Some(end + 1);
        }
        end += if source[end] == b'\\' { 2 } else { 1 };
    }
    Some(source.len())
}
